// SPDX-License-Identifier: AGPL-3.0-or-later
//! Uploads and the files area over the router (docs/specs/m1/06-files.md B3 to B12, B21; T1a,
//! T2 to T11 and T19 for these routes). The permission suite's generated cases for each route
//! (T18, T19) are in `tests/permissions.rs`.
//!
//! The upload tests run against the MinIO in the gate's own Compose project (`SC_CI_MINIO_URL`,
//! as `tests/storage.rs` describes), each under a prefix of its own, and the ones about what is
//! left in storage run against a temporary local directory too. No case reaches the network
//! outside the stack. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use axum::{
    Extension, Router,
    body::{Body, Bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::{BodyExt, Channel, Full};
use object_store::{ObjectStoreExt as _, PutPayload};
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    clock::ManualClock,
    courses,
    files::Sha256Hasher,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles,
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    config::{S3Config, StorageConfig},
    files_cleanup,
    request_org::TEST_ORG_HEADER,
    router, session,
    storage::{Storage, UploadLimits},
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

/// The largest upload these tests accept: small, so a refusal needs no large body.
const MAX_FILE_BYTES: u64 = 64 * 1024;

const HOUR: Duration = Duration::from_secs(60 * 60);

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << >> endobj\n%%EOF\n";

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    hasher.finish().sha256().hex()
}

/// Where a test's files are stored.
#[derive(Clone, Copy, Debug)]
enum Backend {
    Local,
    Minio,
}

/// A directory of its own under the system's temporary directory, removed on drop.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The store of `backend`, under a place of the test's own, and what to drop after the test.
fn storage(backend: Backend) -> (Storage, Option<TempDir>) {
    let (config, dir) = match backend {
        Backend::Local => {
            let dir = std::env::temp_dir().join(format!("stepclimb-files-{}", new_id()));
            std::fs::create_dir_all(&dir).expect("create the directory");
            let config = StorageConfig::Local { root: dir.clone() };
            (config, Some(TempDir(dir)))
        }
        Backend::Minio => {
            let endpoint = std::env::var("SC_CI_MINIO_URL").expect(
                "SC_CI_MINIO_URL names the gate's MinIO: run these tests inside `just ci` or \
                 `python3 bin/ci_gate.py run -- cargo nextest run -p stepclimb-server --test files`",
            );
            let config = StorageConfig::S3(S3Config {
                bucket: "stepclimb".to_owned(),
                prefix: format!("tests/{}", new_id().simple()),
                endpoint: Some(endpoint),
                region: "us-east-1".to_owned(),
                // deploy/compose/compose.yaml's development credentials.
                access_key_id: "stepclimb".to_owned(),
                secret_access_key: "stepclimb-local".to_owned(),
                allow_http: true,
            });
            (config, None)
        }
    };
    let storage = Storage::open(&config, MAX_FILE_BYTES)
        .expect("open the store")
        .expect("storage is configured");
    (storage, dir)
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    #[track_caller]
    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!((self.status, self.code()), (status, code), "{self:?}");
    }

    #[track_caller]
    fn invalid_at(&self, field: &str, code: &str) {
        self.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
        let errors: Vec<(&str, &str)> = self.body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .map(|e| {
                (
                    e["field"].as_str().expect("a field"),
                    e["code"].as_str().expect("a code"),
                )
            })
            .collect();
        assert_eq!(errors, [(field, code)], "{self:?}");
    }

    #[track_caller]
    fn ok(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }

    fn id(&self) -> Uuid {
        self.body["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a UUID")
    }
}

/// An organization with Tower under the root, and a draft course in Tower.
#[derive(Clone, Copy, Debug)]
struct Tree {
    org: Uuid,
    course: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
    storage: Storage,
    _dir: Option<TempDir>,
    a: Tree,
    admin: String,
}

impl Fixture {
    async fn new(pool: PgPool, backend: Backend) -> Self {
        let (storage, dir) = storage(backend);
        Self::with(pool, Some(storage), dir).await
    }

    /// [`Self::new`] with uploads bounded by `limits`.
    async fn limited(pool: PgPool, backend: Backend, limits: UploadLimits) -> Self {
        let (storage, dir) = storage(backend);
        Self::with(pool, Some(storage.with_upload_limits(limits)), dir).await
    }

    async fn with(pool: PgPool, storage: Option<Storage>, dir: Option<TempDir>) -> Self {
        roles::sync_builtin(&pool, SystemTime::now())
            .await
            .expect("sync roles");
        // Real time: the store stamps objects with its own clock, which cleanup compares to
        // this one.
        let clock = ManualClock::new(SystemTime::now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage: storage.clone(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        let slug = format!("org-a-{}", &new_id().simple().to_string()[24..]);
        let a = tree(&pool, &slug, clock.clock().now()).await;
        let mut fixture = Self {
            pool,
            app,
            clock,
            // A store that is never used when the router has none.
            storage: storage.unwrap_or_else(|| self::storage(Backend::Local).0),
            _dir: dir,
            a,
            admin: String::new(),
        };
        fixture.admin = fixture
            .holder(a.org, "org_admin", Scope::Organization)
            .await;
        fixture
    }

    fn now(&self) -> SystemTime {
        self.clock.clock().now()
    }

    async fn holder(&self, org: Uuid, key: &str, at: Scope) -> String {
        let new_user = NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(&self.pool, scope(org), &new_user, self.now())
            .await
            .expect("create user")
            .id;
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(org),
            &assignment,
            self.now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), self.now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn course(&self, org: Uuid, code: &str) -> Uuid {
        let root =
            sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
                .bind(org)
                .fetch_one(&self.pool)
                .await
                .expect("the root");
        let new = courses::NewCourse {
            account_id: root,
            title: format!("Course {code}"),
            code: code.into(),
        };
        courses::create(&self.pool, scope(org), &new, self.now(), Actor::Instance)
            .await
            .expect("create course")
            .id
    }

    fn base(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: &str,
    ) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie)
    }

    /// An upload of `bytes` as `org`'s `cookie`, `query` already encoded.
    async fn upload_as(
        &self,
        org: Uuid,
        cookie: &str,
        course: Uuid,
        query: &str,
        bytes: &[u8],
    ) -> Answer {
        let request = self
            .base(
                Method::POST,
                &format!("/api/v1/courses/{course}/files?{query}"),
                org,
                cookie,
            )
            .header(header::CONTENT_LENGTH, bytes.len())
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(bytes.to_vec()))
            .expect("a request");
        answer(&self.app, request).await
    }

    /// The organization admin of org A uploads `bytes` as `name` into the top folder.
    async fn upload(&self, course: Uuid, name: &str, bytes: &[u8]) -> Answer {
        let query = format!("name={}", encode(name));
        self.upload_as(self.a.org, &self.admin, course, &query, bytes)
            .await
    }

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Answer {
        let mut request = self.base(method, uri, self.a.org, &self.admin);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        answer(
            &self.app,
            request.body(Body::from(body)).expect("a request"),
        )
        .await
    }

    /// Every object key of `org` in the store, relative to the store's prefix, sorted.
    async fn objects(&self, org: Uuid) -> Vec<String> {
        let mut keys = Vec::new();
        let mut pending = vec![self.storage.path(&format!("orgs/{org}"))];
        while let Some(at) = pending.pop() {
            let page = self
                .storage
                .store()
                .list_with_delimiter(Some(&at))
                .await
                .expect("list");
            keys.extend(
                page.objects
                    .iter()
                    .filter_map(|o| self.storage.key_of(&o.location)),
            );
            pending.extend(page.common_prefixes);
        }
        keys.sort();
        keys
    }

    async fn uploads_left(&self, org: Uuid) -> Vec<String> {
        let mut keys = self.objects(org).await;
        keys.retain(|key| key.contains("/uploads/"));
        keys
    }

    async fn count(&self, table: &str, org: Uuid) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table} WHERE org_id = $1"
        )))
        .bind(org)
        .fetch_one(&self.pool)
        .await
        .expect("count rows")
    }

    async fn read_object(&self, key: &str) -> Vec<u8> {
        let got = self
            .storage
            .store()
            .get(&self.storage.path(key))
            .await
            .expect("get");
        got.bytes().await.expect("bytes").to_vec()
    }
}

/// Percent-encode `value` for a query string.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

async fn tree(pool: &PgPool, slug: &str, now: SystemTime) -> Tree {
    let org = organizations::create(pool, slug, slug, now)
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let tower = accounts::create(pool, scope(org), root, "Tower", now, Actor::Instance)
        .await
        .expect("create account")
        .id;
    let new = courses::NewCourse {
        account_id: tower,
        title: "Course S1".into(),
        code: "S1".into(),
    };
    let course = courses::create(pool, scope(org), &new, now, Actor::Instance)
        .await
        .expect("create course")
        .id;
    Tree { org, course }
}

async fn answer(app: &Router, request: Request<Body>) -> Answer {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router never fails");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("the body")
        .to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    Answer {
        status,
        headers,
        body,
    }
}

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

/// B3, B5, B6: an upload stores the bytes the server counted and hashed at the hash's key, under
/// the sniffed type, and answers the course file with its `Location`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_upload_stores_the_bytes_at_their_hash_with_the_sniffed_type(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        let query = format!(
            "folder={}&name={}",
            encode("/Handouts/Week 1"),
            encode("Cafe\u{301}.pdf")
        );
        let created = f.upload_as(t.org, &f.admin, t.course, &query, PDF).await;
        let body = created.ok(StatusCode::CREATED).clone();
        assert_schema("CourseFile", &body);
        let id = created.id();
        assert_eq!(
            created.headers.get(header::LOCATION).unwrap(),
            &format!("/api/v1/course-files/{id}")
        );
        assert_eq!(body["folder"], "/Handouts/Week 1");
        assert_eq!(body["name"], "Caf\u{e9}.pdf", "stored NFC-normalized");
        assert_eq!(body["size"], PDF.len());
        assert_eq!(body["mime"], "application/pdf");
        assert_eq!(body["sha256"], sha256(PDF));
        let key = format!("orgs/{}/files/{}", t.org, sha256(PDF));
        assert_eq!(
            f.objects(t.org).await,
            std::slice::from_ref(&key),
            "{backend:?}"
        );
        assert_eq!(f.read_object(&key).await, PDF);
        let read = f
            .send(Method::GET, &format!("/api/v1/course-files/{id}"), None)
            .await;
        assert_eq!(read.ok(StatusCode::OK), &body);
        // Another organization's state is reused below; start the next backend clean.
        sqlx::query("DELETE FROM course_files")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM files")
            .execute(&pool)
            .await
            .unwrap();
    }
}

/// T5 (B6): the type comes from the bytes, never the name or the client's `Content-Type`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_the_type_is_sniffed_from_the_bytes(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let pdf = f.upload(f.a.course, "x.png", PDF).await;
    assert_eq!(pdf.ok(StatusCode::CREATED)["mime"], "application/pdf");
    let html = b"<!DOCTYPE html><html><script>alert(document.cookie)</script></html>";
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/files?name=page.png", f.a.course),
            f.a.org,
            &f.admin,
        )
        .header(header::CONTENT_LENGTH, html.len())
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(html.to_vec()))
        .unwrap();
    let stored = answer(&f.app, request).await;
    assert_eq!(
        stored.ok(StatusCode::CREATED)["mime"],
        "application/octet-stream"
    );
    let text = f
        .upload(
            f.a.course,
            "notes.txt",
            "Week 1: read chapter 2\n".as_bytes(),
        )
        .await;
    assert_eq!(
        text.ok(StatusCode::CREATED)["mime"],
        "text/plain; charset=utf-8"
    );
}

/// A body that records whether anything read it.
fn watched(bytes: &'static [u8]) -> (Body, Arc<AtomicBool>) {
    let read = Arc::new(AtomicBool::new(false));
    let flag = read.clone();
    let body = Full::new(Bytes::from_static(bytes)).map_frame(move |frame| {
        flag.store(true, Ordering::SeqCst);
        frame
    });
    (Body::new(body), read)
}

/// T2 (B3, B4): the refusals before the body is read.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_an_upload_is_refused_before_its_body_is_read(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let (t, uri) = (
        f.a,
        format!("/api/v1/courses/{}/files?name=a.txt", f.a.course),
    );
    let (body, read) = watched(b"hello");
    let request = f
        .base(Method::POST, &uri, t.org, &f.admin)
        .body(body)
        .unwrap();
    answer(&f.app, request)
        .await
        .expect(StatusCode::LENGTH_REQUIRED, "file.length_required");
    assert!(!read.load(Ordering::SeqCst));
    let (body, read) = watched(b"hello");
    let request = f
        .base(Method::POST, &uri, t.org, &f.admin)
        .header(header::CONTENT_LENGTH, MAX_FILE_BYTES + 1)
        .body(body)
        .unwrap();
    answer(&f.app, request)
        .await
        .expect(StatusCode::PAYLOAD_TOO_LARGE, "file.too_large");
    assert!(!read.load(Ordering::SeqCst), "the body was read");
    let at_max = vec![b'a'; usize::try_from(MAX_FILE_BYTES).unwrap()];
    f.upload(t.course, "max.txt", &at_max)
        .await
        .ok(StatusCode::CREATED);
    let long = "n".repeat(256);
    let nine = "/1/2/3/4/5/6/7/8/9";
    for (query, field, code) in [
        ("name=a%2Fb".to_owned(), "/name", "file.name_invalid"),
        (format!("name={long}"), "/name", "file.name_too_long"),
        (
            "folder=%2Fa%2F..&name=x".to_owned(),
            "/folder",
            "file.folder_invalid",
        ),
        (
            format!("folder={}&name=x", encode(nine)),
            "/folder",
            "file.folder_too_deep",
        ),
    ] {
        let (body, read) = watched(b"hello");
        let request = f
            .base(
                Method::POST,
                &format!("/api/v1/courses/{}/files?{query}", t.course),
                t.org,
                &f.admin,
            )
            .header(header::CONTENT_LENGTH, 5)
            .body(body)
            .unwrap();
        answer(&f.app, request).await.invalid_at(field, code);
        assert!(!read.load(Ordering::SeqCst), "{query}");
    }
    let (body, read) = watched(b"hello");
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/files?name=max.txt", t.course),
            t.org,
            &f.admin,
        )
        .header(header::CONTENT_LENGTH, 5)
        .body(body)
        .unwrap();
    answer(&f.app, request)
        .await
        .expect(StatusCode::CONFLICT, "file.name_taken");
    assert!(!read.load(Ordering::SeqCst));
    assert!(f.uploads_left(t.org).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_takes_no_upload_or_change(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let t = f.a;
    let id = f.upload(t.course, "a.txt", b"before").await.id();
    for status in [courses::Status::Published, courses::Status::Archived] {
        courses::set_status(
            &f.pool,
            scope(t.org),
            t.course,
            status,
            f.now(),
            Actor::Instance,
        )
        .await
        .unwrap();
    }
    f.upload(t.course, "b.txt", b"after")
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    let path = format!("/api/v1/course-files/{id}");
    let renamed = f
        .send(Method::PATCH, &path, Some(json!({ "name": "c.txt" })))
        .await;
    renamed.expect(StatusCode::CONFLICT, "course.archived");
    f.send(Method::DELETE, &path, None)
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    let rename = json!({ "from": "/x", "to": "/y" });
    let uri = format!("/api/v1/courses/{}/folders/rename", t.course);
    f.send(Method::POST, &uri, Some(rename))
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    // Reading still works.
    f.send(Method::GET, &path, None).await.ok(StatusCode::OK);
}

/// T1a (B1a): without storage an allowed upload answers 503 naming the setting, an anonymous
/// one still 401, and the routes that touch rows only keep working.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1a_without_storage_an_upload_answers_503_and_listing_works(pool: PgPool) {
    let f = Fixture::with(pool, None, None).await;
    let t = f.a;
    let refused = f.upload(t.course, "a.txt", b"hello").await;
    refused.expect(StatusCode::SERVICE_UNAVAILABLE, "storage.not_configured");
    assert!(
        refused.body["detail"]
            .as_str()
            .unwrap()
            .contains("STEPCLIMB_STORAGE_URL")
    );
    let anonymous = Request::post(format!("/api/v1/courses/{}/files?name=a.txt", t.course))
        .header(TEST_ORG_HEADER, t.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_LENGTH, 5)
        .body(Body::from("hello"))
        .unwrap();
    answer(&f.app, anonymous)
        .await
        .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    let listed = f
        .send(
            Method::GET,
            &format!("/api/v1/courses/{}/files", t.course),
            None,
        )
        .await;
    assert_eq!(listed.ok(StatusCode::OK)["items"], json!([]));
}

/// T3 (B4, B7, B22): the `storage` limit counts new bytes only.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_the_storage_limit_counts_only_new_bytes(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let t = f.a;
    limit_storage(&f.pool, t.org, 1000).await;
    let nine_hundred = vec![b'n'; 900];
    f.upload(t.course, "900.txt", &nine_hundred)
        .await
        .ok(StatusCode::CREATED);
    let (body, read) = watched(&[b'm'; 200]);
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/files?name=200.txt", t.course),
            t.org,
            &f.admin,
        )
        .header(header::CONTENT_LENGTH, 200)
        .body(body)
        .unwrap();
    let full = answer(&f.app, request).await;
    full.expect(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(
        (full.body["limit"].as_str(), full.body["max"].as_u64()),
        (Some("storage"), Some(1000))
    );
    assert!(
        !read.load(Ordering::SeqCst),
        "refused before the body was read"
    );
    let other = f.course(t.org, "S2").await;
    f.upload(other, "again.txt", &nine_hundred)
        .await
        .ok(StatusCode::CREATED);
    assert_eq!(f.count("files", t.org).await, 1);
    // A body of a stored file's length passes the first check; a new one is refused once its
    // hash shows it is new, and leaves nothing behind.
    let new_900 = vec![b'x'; 900];
    let refused = f.upload(other, "new.txt", &new_900).await;
    refused.expect(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(f.count("files", t.org).await, 1);
    assert_eq!(f.objects(t.org).await.len(), 1);
}

/// Put `org` on a test plan whose `storage` limit is `bytes`, by SQL (M1 has no API for it).
async fn limit_storage(pool: &PgPool, org: Uuid, bytes: i64) {
    let plan = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, limits) VALUES ($1, $2, jsonb_build_object('storage', $3::bigint))",
    )
    .bind(plan)
    .bind(format!("test_{}", plan.simple()))
    .bind(bytes)
    .execute(pool)
    .await
    .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(org)
        .bind(plan)
        .execute(pool)
        .await
        .expect("move to the plan");
}

/// T4 (B5): a body shorter or longer than its `Content-Length` answers 400 and leaves no object.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_a_body_of_another_length_leaves_nothing(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        for (said, sent) in [(10_u64, &b"short"[..]), (3, b"longer")] {
            let request = f
                .base(
                    Method::POST,
                    &format!("/api/v1/courses/{}/files?name=a.txt", t.course),
                    t.org,
                    &f.admin,
                )
                .header(header::CONTENT_LENGTH, said)
                .body(Body::from(sent.to_vec()))
                .unwrap();
            answer(&f.app, request)
                .await
                .expect(StatusCode::BAD_REQUEST, "request.malformed");
        }
        assert_eq!(f.objects(t.org).await, Vec::<String>::new(), "{backend:?}");
        assert_eq!(f.count("files", t.org).await, 0);
        assert_eq!(f.count("course_files", t.org).await, 0);
    }
}

/// T6 (B7): the same bytes are stored once per organization, uploaded one after the other or
/// at once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_the_same_bytes_make_one_file_and_one_object(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let t = f.a;
    let other = f.course(t.org, "S2").await;
    let first = f.upload(t.course, "a.pdf", PDF).await;
    let second = f.upload(other, "b.pdf", PDF).await;
    let (first, second) = (
        first.ok(StatusCode::CREATED),
        second.ok(StatusCode::CREATED),
    );
    assert_eq!(first["sha256"], second["sha256"]);
    assert_eq!(f.count("files", t.org).await, 1);
    assert_eq!(f.count("course_files", t.org).await, 2);
    assert_eq!(f.objects(t.org).await.len(), 1);
    // Two uploads of new identical bytes at once, on two connections.
    let bytes = b"uploaded twice at once".repeat(100);
    let (x, y) = tokio::join!(
        f.upload(t.course, "x.txt", &bytes),
        f.upload(other, "y.txt", &bytes)
    );
    assert_eq!(
        (x.status, y.status),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{x:?} {y:?}"
    );
    assert_eq!(f.count("files", t.org).await, 2);
    let key = format!("orgs/{}/files/{}", t.org, sha256(&bytes));
    assert_eq!(f.read_object(&key).await, bytes);
    assert!(f.uploads_left(t.org).await.is_empty());
    // Another organization's copy is its own.
    let b = tree(&f.pool, "org-b", f.now()).await;
    let admin_b = f.holder(b.org, "org_admin", Scope::Organization).await;
    f.upload_as(b.org, &admin_b, b.course, "name=a.pdf", PDF)
        .await
        .ok(StatusCode::CREATED);
    assert_eq!(f.count("files", b.org).await, 1);
    assert_eq!(
        f.objects(b.org).await,
        [format!("orgs/{}/files/{}", b.org, sha256(PDF))]
    );
}

/// Make inserting a course file named `fail.txt` fail, by a trigger: the failure B7's cleanup
/// must handle, after the copy and before the commit.
async fn fail_course_file_named_fail(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE FUNCTION test_fail_upload() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
             IF NEW.name = 'fail.txt' THEN RAISE EXCEPTION 'injected failure'; END IF;
             RETURN NEW;
         END $$;
         CREATE TRIGGER test_fail_upload BEFORE INSERT ON course_files
             FOR EACH ROW EXECUTE FUNCTION test_fail_upload();",
    )
    .execute(pool)
    .await
    .expect("create the trigger");
}

/// T7 (B7): a failure after the copy and before the commit leaves no row and no object for a
/// new hash, and keeps the object of a hash the organization already has.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_failed_upload_leaves_no_row_and_no_new_object(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        if matches!(backend, Backend::Minio) {
            fail_course_file_named_fail(&f.pool).await;
        }
        let failed = f
            .upload(t.course, "fail.txt", b"new bytes nobody has")
            .await;
        failed.expect(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
        assert_eq!(f.count("files", t.org).await, 0);
        assert_eq!(f.objects(t.org).await, Vec::<String>::new(), "{backend:?}");
        f.upload(t.course, "kept.pdf", PDF)
            .await
            .ok(StatusCode::CREATED);
        let failed = f.upload(t.course, "fail.txt", PDF).await;
        failed.expect(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
        let key = format!("orgs/{}/files/{}", t.org, sha256(PDF));
        assert_eq!(
            f.objects(t.org).await,
            std::slice::from_ref(&key),
            "{backend:?}"
        );
        assert_eq!(f.read_object(&key).await, PDF);
        assert_eq!(f.count("files", t.org).await, 1);
        sqlx::query("DELETE FROM course_files")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM files")
            .execute(&pool)
            .await
            .unwrap();
    }
}

/// T8 (B8, B21): cleanup deletes a day-old temporary object and a file whose last course file
/// went a day ago, with its object, and an object no row names; it keeps a file another course
/// still uses and one unreferenced for 23 hours.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_cleanup_deletes_what_has_been_unused_for_a_day(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        sqlx::query("DELETE FROM course_files")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM files")
            .execute(&pool)
            .await
            .unwrap();
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        let other = f.course(t.org, "S2").await;
        let gone = f
            .upload(t.course, "gone.txt", b"deleted a day ago")
            .await
            .id();
        let shared = f
            .upload(t.course, "shared.txt", b"still used elsewhere")
            .await
            .id();
        f.upload(other, "shared.txt", b"still used elsewhere")
            .await
            .ok(StatusCode::CREATED);
        let stale = f.storage.upload_path(scope(t.org), new_id());
        f.storage
            .store()
            .put(&stale, PutPayload::from_static(b"crashed"))
            .await
            .unwrap();
        let orphan = format!("orgs/{}/files/{}", t.org, sha256(b"no row"));
        let orphan_path = f.storage.path(&orphan);
        f.storage
            .store()
            .put(&orphan_path, PutPayload::from_static(b"no row"))
            .await
            .unwrap();
        for id in [gone, shared] {
            f.send(Method::DELETE, &format!("/api/v1/course-files/{id}"), None)
                .await
                .ok(StatusCode::NO_CONTENT);
        }
        let recent = f
            .upload(t.course, "recent.txt", b"deleted an hour later")
            .await
            .id();
        f.clock.advance(HOUR);
        f.send(
            Method::DELETE,
            &format!("/api/v1/course-files/{recent}"),
            None,
        )
        .await
        .ok(StatusCode::NO_CONTENT);
        // 24 hours and a minute after the first deletes, 23 after the last one.
        let at = f.now() + 23 * HOUR + Duration::from_secs(60);
        let report = files_cleanup::run(&f.pool, Some(&f.storage), at)
            .await
            .unwrap();
        assert_eq!(
            (
                report.uploads_deleted,
                report.files_deleted,
                report.orphans_deleted,
                report.failed
            ),
            (1, 1, 1, 0),
            "{backend:?}"
        );
        let mut kept = [b"still used elsewhere".as_slice(), b"deleted an hour later"]
            .map(|bytes| format!("orgs/{}/files/{}", t.org, sha256(bytes)));
        kept.sort();
        assert_eq!(f.objects(t.org).await, kept, "{backend:?}");
        assert_eq!(f.count("files", t.org).await, 2);
        // Run again: nothing more is old enough.
        let again = files_cleanup::run(&f.pool, Some(&f.storage), at)
            .await
            .unwrap();
        assert_eq!(again, files_cleanup::Report::default());
        let later = files_cleanup::run(&f.pool, Some(&f.storage), at + HOUR)
            .await
            .unwrap();
        assert_eq!(later.files_deleted, 1);
        let still_used = format!("orgs/{}/files/{}", t.org, sha256(b"still used elsewhere"));
        assert_eq!(f.objects(t.org).await, [still_used]);
    }
}

/// B1a: without storage, cleanup deletes rows and no object.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_without_storage_deletes_rows_only(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let id = f.upload(t.course, "a.txt", b"bytes").await.id();
    f.send(Method::DELETE, &format!("/api/v1/course-files/{id}"), None)
        .await
        .ok(StatusCode::NO_CONTENT);
    let at = f.now() + 25 * HOUR;
    let report = files_cleanup::run(&f.pool, None, at).await.unwrap();
    assert_eq!((report.files_deleted, report.orphans_deleted), (1, 0));
    assert_eq!(f.objects(t.org).await.len(), 1);
    // Once storage is back, the object no row names goes.
    let report = files_cleanup::run(&f.pool, Some(&f.storage), at)
        .await
        .unwrap();
    assert_eq!(report.orphans_deleted, 1);
    assert!(f.objects(t.org).await.is_empty());
}

/// T9 (B10): a folder rename moves the folder and everything below it, or nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_folder_rename_moves_everything_below_or_nothing(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    for (folder, name) in [("/a", "x"), ("/a/c", "y"), ("/ab", "z"), ("/b2", "x")] {
        let query = format!("folder={}&name={name}", encode(folder));
        f.upload_as(t.org, &f.admin, t.course, &query, name.as_bytes())
            .await
            .ok(StatusCode::CREATED);
    }
    let uri = format!("/api/v1/courses/{}/folders/rename", t.course);
    let moved = f
        .send(
            Method::POST,
            &uri,
            Some(json!({ "from": "/a", "to": "/b" })),
        )
        .await;
    assert_eq!(moved.ok(StatusCode::OK), &json!({ "moved": 2 }));
    assert_schema("FolderRenamed", &moved.body);
    let before = folders(&f).await;
    assert_eq!(before, ["/ab/z", "/b/c/y", "/b/x", "/b2/x"]);
    let collided = f
        .send(
            Method::POST,
            &uri,
            Some(json!({ "from": "/b", "to": "/b2" })),
        )
        .await;
    collided.expect(StatusCode::CONFLICT, "file.name_taken");
    assert_eq!(folders(&f).await, before);
    let missing = f
        .send(
            Method::POST,
            &uri,
            Some(json!({ "from": "/none", "to": "/x" })),
        )
        .await;
    missing.expect(StatusCode::NOT_FOUND, "folder.not_found");
    let root = f
        .send(Method::POST, &uri, Some(json!({ "from": "/", "to": "/x" })))
        .await;
    root.invalid_at("/from", "file.folder_root");
    let deep = json!({ "from": "/b", "to": "/1/2/3/4/5/6/7/8" });
    f.send(Method::POST, &uri, Some(deep))
        .await
        .invalid_at("/to", "file.folder_too_deep");
    let bad = f
        .send(
            Method::POST,
            &uri,
            Some(json!({ "from": "/b", "to": "/x/../y" })),
        )
        .await;
    bad.invalid_at("/to", "file.folder_invalid");
}

async fn folders(f: &Fixture) -> Vec<String> {
    sqlx::query_scalar("SELECT folder || '/' || name FROM course_files ORDER BY 1")
        .fetch_all(&f.pool)
        .await
        .expect("read course files")
}

/// B10: a course file is renamed or moved within its course.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_file_is_renamed_and_moved(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let id = f.upload(t.course, "a.txt", b"a").await.id();
    f.upload(t.course, "b.txt", b"b")
        .await
        .ok(StatusCode::CREATED);
    let path = format!("/api/v1/course-files/{id}");
    let taken = f
        .send(Method::PATCH, &path, Some(json!({ "name": "b.txt" })))
        .await;
    taken.expect(StatusCode::CONFLICT, "file.name_taken");
    f.clock.advance(HOUR);
    let moved = f
        .send(
            Method::PATCH,
            &path,
            Some(json!({ "folder": "/notes", "name": "c.txt" })),
        )
        .await;
    let body = moved.ok(StatusCode::OK).clone();
    assert_schema("CourseFile", &body);
    assert_eq!(
        (body["folder"].as_str(), body["name"].as_str()),
        (Some("/notes"), Some("c.txt"))
    );
    assert_ne!(body["updated_at"], body["created_at"]);
    let bad = f
        .send(Method::PATCH, &path, Some(json!({ "name": "." })))
        .await;
    bad.invalid_at("/name", "file.name_invalid");
    let null = f
        .send(Method::PATCH, &path, Some(json!({ "name": null })))
        .await;
    null.expect(StatusCode::BAD_REQUEST, "request.malformed");
}

/// T10 (B11): a course file a `file` item uses cannot be deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_a_course_file_in_use_is_not_deleted(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let id = f.upload(t.course, "handout.pdf", PDF).await.id();
    let module = stepclimb_core::modules::create(&f.pool, scope(t.org), t.course, "M", f.now())
        .await
        .unwrap()
        .id;
    // `file` items become creatable with spec 06 I3; the row is inserted directly.
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, title)
         VALUES ($1, $2, $3, $4, 0, 'file', $5, 'Handout')",
    )
    .bind(new_id())
    .bind(t.org)
    .bind(t.course)
    .bind(module)
    .bind(id)
    .execute(&f.pool)
    .await
    .unwrap();
    let path = format!("/api/v1/course-files/{id}");
    f.send(Method::DELETE, &path, None)
        .await
        .expect(StatusCode::CONFLICT, "file.in_use");
    sqlx::query("DELETE FROM module_items")
        .execute(&f.pool)
        .await
        .unwrap();
    f.send(Method::DELETE, &path, None)
        .await
        .ok(StatusCode::NO_CONTENT);
    f.send(Method::GET, &path, None)
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    // The bytes stay until cleanup.
    assert_eq!(f.count("files", t.org).await, 1);
}

/// T11 (B12): the listing shows sub-folders, then files, each by name, page by page.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_folder_lists_folders_then_files_page_by_page(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    for (folder, name) in [
        ("/", "b.txt"),
        ("/", "A.txt"),
        ("/zeta", "1"),
        ("/alpha/deep", "2"),
    ] {
        let query = format!("folder={}&name={name}", encode(folder));
        f.upload_as(t.org, &f.admin, t.course, &query, name.as_bytes())
            .await
            .ok(StatusCode::CREATED);
    }
    let uri = format!("/api/v1/courses/{}/files?limit=3", t.course);
    let first = f.send(Method::GET, &uri, None).await;
    let page = first.ok(StatusCode::OK).clone();
    let entries = page["items"].as_array().unwrap();
    for entry in entries {
        assert_schema("FolderEntry", entry);
    }
    assert_eq!(
        entries[..2],
        [
            json!({ "kind": "folder", "path": "/alpha", "name": "alpha" }),
            json!({ "kind": "folder", "path": "/zeta", "name": "zeta" }),
        ]
    );
    assert_eq!(entries[2]["file"]["name"], "A.txt");
    let cursor = page["next_cursor"].as_str().expect("a next page");
    let rest = f
        .send(Method::GET, &format!("{uri}&cursor={cursor}"), None)
        .await;
    let rest = rest.ok(StatusCode::OK);
    assert_eq!(rest["items"].as_array().unwrap().len(), 1);
    assert_eq!(rest["items"][0]["file"]["name"], "b.txt");
    assert_eq!(rest["next_cursor"], Value::Null);
    let inside = f
        .send(
            Method::GET,
            &format!("/api/v1/courses/{}/files?folder=%2Falpha", t.course),
            None,
        )
        .await;
    assert_eq!(
        inside.ok(StatusCode::OK)["items"],
        json!([{ "kind": "folder", "path": "/alpha/deep", "name": "deep" }])
    );
    let bad = f
        .send(
            Method::GET,
            &format!("/api/v1/courses/{}/files?folder=x", t.course),
            None,
        )
        .await;
    bad.invalid_at("/folder", "file.folder_invalid");
}

/// T19 for uploads: org A's admin uploading into org B's course gets 404 and writes nothing to
/// org B's rows or storage, nor any temporary object.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19_an_upload_into_another_organizations_course_writes_nothing(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let b = tree(&f.pool, "org-b", f.now()).await;
    let crossed = f.upload(b.course, "x.pdf", PDF).await;
    crossed.expect(StatusCode::NOT_FOUND, "course.not_found");
    assert!(f.objects(b.org).await.is_empty());
    assert!(f.objects(f.a.org).await.is_empty());
    assert_eq!(
        f.count("files", b.org).await + f.count("files", f.a.org).await,
        0
    );
}

/// B7, B21: an object whose upload holds the hash lock between its copy and its commit is not
/// an orphan. Cleanup waits for the lock, sees the committed row, and keeps the object.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_keeps_the_object_of_an_upload_still_committing(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let bytes = b"copied, row not yet committed";
    let key = format!("orgs/{}/files/{}", t.org, sha256(bytes));
    let path = f.storage.path(&key);
    f.storage
        .store()
        .put(&path, PutPayload::from_static(bytes))
        .await
        .expect("put the copy");
    // The upload's transaction: the hash lock, then the row, not yet committed.
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    let hashed = hasher.finish();
    let mut upload = f.pool.begin().await.expect("begin");
    stepclimb_core::files::lock_hash(&mut upload, scope(t.org), hashed.sha256())
        .await
        .expect("lock the hash");
    let new = stepclimb_core::files::NewFile {
        hashed,
        mime: "text/plain; charset=utf-8".into(),
    };
    stepclimb_core::files::insert(&mut *upload, scope(t.org), &new, f.now())
        .await
        .expect("insert the row");
    let (pool, storage, at) = (f.pool.clone(), f.storage.clone(), f.now());
    let cleanup = tokio::spawn(async move { files_cleanup::run(&pool, Some(&storage), at).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!cleanup.is_finished(), "cleanup did not wait for the lock");
    upload.commit().await.expect("commit");
    let report = cleanup.await.expect("the task").expect("the run");
    assert_eq!(report.orphans_deleted, 0);
    assert_eq!(f.read_object(&key).await, bytes);
}

/// An upload of `length` bytes whose body the returned sender feeds, and the request.
fn held_open(
    f: &Fixture,
    name: &str,
    length: usize,
) -> (http_body_util::channel::Sender<Bytes>, Request<Body>) {
    let (sender, body) = Channel::<Bytes>::new(1);
    let uri = format!("/api/v1/courses/{}/files?name={name}", f.a.course);
    let request = f
        .base(Method::POST, &uri, f.a.org, &f.admin)
        .header(header::CONTENT_LENGTH, length)
        .body(Body::new(body))
        .expect("a request");
    (sender, request)
}

/// B5: a body that stops sending is refused after the idle deadline, its multipart upload
/// aborted, instead of holding a place for uploads forever.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stalled_upload_is_refused_and_leaves_nothing(pool: PgPool) {
    let limits = UploadLimits {
        idle: Duration::from_millis(300),
        in_flight: 1,
    };
    for backend in [Backend::Minio, Backend::Local] {
        let f = Fixture::limited(pool.clone(), backend, limits).await;
        let t = f.a;
        let (mut sender, request) = held_open(&f, "stalled.txt", 10);
        sender
            .send_data(Bytes::from_static(b"abc"))
            .await
            .expect("the first frame");
        let stalled = tokio::time::timeout(Duration::from_secs(10), answer(&f.app, request))
            .await
            .expect("the server answered a stalled body");
        stalled.expect(StatusCode::REQUEST_TIMEOUT, "file.body_timeout");
        drop(sender);
        assert_eq!(f.objects(t.org).await, Vec::<String>::new(), "{backend:?}");
        assert_eq!(f.count("files", t.org).await, 0);
        // The stalled upload gave its place back.
        f.upload(t.course, "next.txt", b"next")
            .await
            .ok(StatusCode::CREATED);
    }
}

/// B5: one server streams at most `in_flight` uploads at once; one more answers 503 until a
/// place is free.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn uploads_past_the_in_flight_limit_answer_503(pool: PgPool) {
    let limits = UploadLimits {
        idle: Duration::from_secs(10),
        in_flight: 1,
    };
    let f = Fixture::limited(pool, Backend::Local, limits).await;
    let t = f.a;
    let (mut sender, request) = held_open(&f, "first.txt", 5);
    let app = f.app.clone();
    let first = tokio::spawn(async move { answer(&app, request).await });
    // Wait until the first upload holds the place: it has read its first frame.
    sender
        .send_data(Bytes::from_static(b"fir"))
        .await
        .expect("a frame");
    sender
        .send_data(Bytes::from_static(b"s"))
        .await
        .expect("a frame");
    let busy = f.upload(t.course, "second.txt", b"second").await;
    busy.expect(StatusCode::SERVICE_UNAVAILABLE, "file.uploads_busy");
    sender
        .send_data(Bytes::from_static(b"t"))
        .await
        .expect("the last frame");
    drop(sender);
    first
        .await
        .expect("the first upload")
        .ok(StatusCode::CREATED);
    f.upload(t.course, "second.txt", b"second")
        .await
        .ok(StatusCode::CREATED);
    assert_eq!(f.count("course_files", t.org).await, 2);
}
