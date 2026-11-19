// SPDX-License-Identifier: AGPL-3.0-or-later
//! Signed delivery and file items over the router (docs/specs/m1/06-files.md B13 to B17; T1a,
//! T12 to T14 and T19 for these routes). The permission suite's generated cases for the two URL
//! routes (T18, T19) are in `tests/permissions.rs`, and their lock suite entries in
//! `tests/lock_suite/mod.rs`.
//!
//! Files are stored in a temporary local directory, and the range tests also run against the
//! MinIO in the gate's own Compose project (`SC_CI_MINIO_URL`). Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use object_store::ObjectStoreExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    clock::ManualClock,
    courses, enrollments,
    id::{Uuid, new_id},
    items, modules, organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles,
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    config::{S3Config, StorageConfig},
    file_delivery::{PDF_POLICY, SANDBOX_POLICY},
    request_org::TEST_ORG_HEADER,
    router, session,
    storage::Storage,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// The same key id with other key bytes: a server whose ring is another organization's.
const OTHER_KEYS: &str = "test:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=";

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << >> endobj\n%%EOF\n";

/// A PNG signature and the start of an IHDR chunk: enough for the sniffer.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0";

const HTML: &[u8] = b"<!doctype html><script>alert(1)</script>";

/// 2025-11-13T02:13:20Z, a whole second.
fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

#[derive(Clone, Copy, Debug)]
enum Backend {
    Local,
    Minio,
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn storage(backend: Backend) -> (Storage, Option<TempDir>) {
    let (config, dir) = match backend {
        Backend::Local => {
            let dir = std::env::temp_dir().join(format!("stepclimb-delivery-{}", new_id()));
            std::fs::create_dir_all(&dir).expect("create the directory");
            (
                StorageConfig::Local { root: dir.clone() },
                Some(TempDir(dir)),
            )
        }
        Backend::Minio => {
            let endpoint = std::env::var("SC_CI_MINIO_URL").expect(
                "SC_CI_MINIO_URL names the gate's MinIO: run these tests inside `just ci` or \
                 `python3 bin/ci_gate.py run -- cargo nextest run -p stepclimb-server --test \
                 file_delivery`",
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
    let storage = Storage::open(&config, 64 * 1024)
        .expect("open the store")
        .expect("storage is configured");
    (storage, dir)
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    fn code(&self) -> String {
        self.json()["code"].as_str().unwrap_or("").to_owned()
    }

    #[track_caller]
    fn expect(&self, status: StatusCode, code: &str) {
        assert_eq!(
            (self.status, self.code().as_str()),
            (status, code),
            "{self:?}"
        );
    }

    #[track_caller]
    fn ok(&self) -> Value {
        assert_eq!(self.status, StatusCode::OK, "{self:?}");
        self.json()
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

/// An organization with a published course in its root account, a published module and its
/// signed-in users.
#[derive(Clone, Copy, Debug)]
struct Org {
    id: Uuid,
    course: Uuid,
    module: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
    _dir: Option<TempDir>,
    a: Org,
    admin: String,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let (storage, dir) = storage(Backend::Local);
        Self::with(pool, Some(storage), dir, KEYS).await
    }

    async fn with(
        pool: PgPool,
        storage: Option<Storage>,
        dir: Option<TempDir>,
        keys: &str,
    ) -> Self {
        roles::sync_builtin(&pool, start())
            .await
            .expect("sync roles");
        let clock = ManualClock::new(start());
        let app = app(&pool, &clock, storage, keys);
        let a = org(
            &pool,
            &format!("org-a-{}", &new_id().simple().to_string()[24..]),
        )
        .await;
        let mut fixture = Self {
            pool,
            app,
            clock,
            _dir: dir,
            a,
            admin: String::new(),
        };
        fixture.admin = fixture.holder(a.id, "org_admin", Scope::Organization).await;
        fixture
    }

    async fn user(&self, org: Uuid) -> (Uuid, String) {
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
        let user = users::create(&self.pool, scope(org), &new_user, start())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), start())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (user, cookie.split(';').next().expect("a pair").to_owned())
    }

    async fn role(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    async fn holder(&self, org: Uuid, key: &str, at: Scope) -> String {
        let (user, cookie) = self.user(org).await;
        let assignment = NewAssignment {
            user_id: user,
            role_id: self.role(key).await,
            scope: at,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(org),
            &assignment,
            start(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
        cookie
    }

    /// A user enrolled in `o`'s course with the built-in role `key`: id and cookie.
    async fn enrolled(&self, o: Org, key: &str, observed: Vec<Uuid>) -> (Uuid, String) {
        let (user, cookie) = self.user(o.id).await;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: self.role(key).await,
            source: Source::Local,
            external_id: None,
            observed_user_ids: observed,
        };
        enrollments::create(
            &self.pool,
            scope(o.id),
            o.course,
            &new,
            start(),
            Actor::Instance,
        )
        .await
        .expect("enroll");
        (user, cookie)
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        org: Uuid,
        cookie: Option<&str>,
        body: Option<Value>,
        headers: &[(header::HeaderName, &str)],
    ) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        answer(
            &self.app,
            request.body(Body::from(body)).expect("a request"),
        )
        .await
    }

    /// `bytes` uploaded as `name` into `o`'s course by its admin `admin`: the course file's id.
    async fn upload_to(&self, o: Org, admin: &str, name: &str, bytes: &[u8]) -> Uuid {
        let uri = format!("/api/v1/courses/{}/files?name={}", o.course, encode(name));
        let request = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(TEST_ORG_HEADER, o.id.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, admin)
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes.to_vec()))
            .expect("a request");
        let answer = answer(&self.app, request).await;
        assert_eq!(answer.status, StatusCode::CREATED, "{answer:?}");
        answer.json()["id"]
            .as_str()
            .expect("an id")
            .parse()
            .expect("a UUID")
    }

    async fn upload(&self, name: &str, bytes: &[u8]) -> Uuid {
        self.upload_to(self.a, &self.admin, name, bytes).await
    }

    /// A published `file` item of `o`'s module naming `course_file`.
    async fn file_item(&self, o: Org, course_file: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::File,
            title: "Handout".into(),
            ref_id: Some(course_file),
            url: None,
            position: None,
        };
        let item = items::create(&self.pool, scope(o.id), o.module, &new, start())
            .await
            .expect("create item")
            .id;
        let publish = items::ItemPatch {
            published: Some(true),
            ..Default::default()
        };
        items::update(&self.pool, scope(o.id), item, &publish, start())
            .await
            .expect("publish item");
        item
    }

    /// A signed URL of the item `item` for `cookie`: its path (`/files/<token>`) and answer.
    async fn item_url(&self, item: Uuid, cookie: &str, disposition: &str) -> Answer {
        let uri = format!("/api/v1/items/{item}/file-url");
        let body = json!({ "disposition": disposition });
        self.send(Method::POST, &uri, self.a.id, Some(cookie), Some(body), &[])
            .await
    }

    /// The organization admin's signed URL path of the course file `id`.
    async fn file_path(&self, id: Uuid, disposition: &str) -> String {
        let uri = format!("/api/v1/course-files/{id}/url");
        let body = json!({ "disposition": disposition });
        let answer = self
            .send(
                Method::POST,
                &uri,
                self.a.id,
                Some(&self.admin),
                Some(body),
                &[],
            )
            .await;
        path_of(&answer.ok())
    }

    /// `path` fetched in org A without a session, with `headers`.
    async fn fetch(&self, path: &str, headers: &[(header::HeaderName, &str)]) -> Answer {
        self.send(Method::GET, path, self.a.id, None, None, headers)
            .await
    }
}

/// The router of a server with `storage` and the key ring `keys`, on `clock`.
fn app(pool: &PgPool, clock: &ManualClock, storage: Option<Storage>, keys: &str) -> Router {
    let state = AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse(keys).expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        passwords: Default::default(),
        storage,
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    };
    router(state).layer(Extension(ConnectInfo(CLIENT)))
}

/// The path of a signed URL answer: what follows the public URL.
fn path_of(answer: &Value) -> String {
    let url = answer["url"].as_str().expect("a url");
    url.strip_prefix("https://lms.example.org")
        .expect("the public URL")
        .to_owned()
}

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

async fn org(pool: &PgPool, slug: &str) -> Org {
    let id = organizations::create(pool, slug, slug, start())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the root");
    let tower = accounts::create(pool, scope(id), root, "Tower", start(), Actor::Instance)
        .await
        .expect("create account")
        .id;
    let new = courses::NewCourse {
        account_id: tower,
        title: "Course S1".into(),
        code: "S1".into(),
    };
    let course = courses::create(pool, scope(id), &new, start(), Actor::Instance)
        .await
        .expect("create course")
        .id;
    courses::set_status(
        pool,
        scope(id),
        course,
        courses::Status::Published,
        start(),
        Actor::Instance,
    )
    .await
    .expect("publish the course");
    let module = modules::create(pool, scope(id), course, "Module", start())
        .await
        .expect("create module")
        .id;
    let publish = modules::ModulePatch {
        published: Some(true),
        ..Default::default()
    };
    modules::update(pool, scope(id), module, &publish, start())
        .await
        .expect("publish module");
    Org { id, course, module }
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
        .to_bytes()
        .to_vec();
    Answer {
        status,
        headers,
        bytes,
    }
}

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

/// B13: a `file` item is created through the API with a course file of its course; without a
/// `ref_id`, with another course's file or another organization's, it is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_a_file_item_names_a_course_file_of_its_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let mine = f.upload("handout.pdf", PDF).await;
    let other = org(&f.pool, "org-b").await;
    let other_admin = f.holder(other.id, "org_admin", Scope::Organization).await;
    let theirs = f.upload_to(other, &other_admin, "theirs.pdf", PDF).await;
    let course_b = {
        let root: Uuid =
            sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
                .bind(a.id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        let new = courses::NewCourse {
            account_id: root,
            title: "Course S2".into(),
            code: "S2".into(),
        };
        courses::create(&f.pool, scope(a.id), &new, start(), Actor::Instance)
            .await
            .unwrap()
            .id
    };
    let second = Org {
        course: course_b,
        ..a
    };
    let elsewhere = f.upload_to(second, &f.admin, "elsewhere.pdf", PDF).await;
    let uri = format!("/api/v1/modules/{}/items", a.module);
    let create =
        |ref_id: Option<Uuid>| json!({ "kind": "file", "title": "Handout", "ref_id": ref_id });

    let made = f
        .send(
            Method::POST,
            &uri,
            a.id,
            Some(&f.admin),
            Some(create(Some(mine))),
            &[],
        )
        .await;
    assert_eq!(made.status, StatusCode::CREATED, "{made:?}");
    let item = made.json();
    assert_schema("Item", &item);
    assert_eq!(
        (item["kind"].as_str(), item["ref_id"].as_str()),
        (Some("file"), Some(mine.to_string().as_str()))
    );

    for refused in [elsewhere, theirs, new_id()] {
        let answer = f
            .send(
                Method::POST,
                &uri,
                a.id,
                Some(&f.admin),
                Some(create(Some(refused))),
                &[],
            )
            .await;
        answer.expect(StatusCode::NOT_FOUND, "file.not_found");
    }
    let answer = f
        .send(
            Method::POST,
            &uri,
            a.id,
            Some(&f.admin),
            Some(create(None)),
            &[],
        )
        .await;
    answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(answer.json()["errors"][0]["code"], "item.ref_id_required");
    // The course file an item names cannot be deleted (B11).
    f.send(
        Method::DELETE,
        &format!("/api/v1/course-files/{mine}"),
        a.id,
        Some(&f.admin),
        None,
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT, "file.in_use");
}

/// T12 (B14, spec 04 B13): a Student asking for the URL of a locked file item gets 403
/// `item.locked`; of an unlocked one, a URL that serves the bytes; `/course-files/{id}/url`
/// answers them 403 `permission.denied`. An Observer, who has no progress, is held to the lock
/// as a learner with nothing done. An unpublished item answers 404, and an item that is not a
/// file 422.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_a_learner_gets_a_file_url_only_for_an_unlocked_file_item(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let file = f.upload("handout.pdf", PDF).await;
    let item = f.file_item(a, file).await;
    let (learner, student) = f.enrolled(a, "student", Vec::new()).await;
    let (_, observer) = f.enrolled(a, "observer", vec![learner]).await;
    let designer = f.holder(a.id, "course_designer", Scope::Organization).await;

    let dated = modules::RulesPatch {
        unlock_at: Some(Some(start() + Duration::from_secs(3_600))),
        ..Default::default()
    };
    modules::set_rules(&f.pool, scope(a.id), a.module, &dated, start())
        .await
        .unwrap();
    for cookie in [&student, &observer] {
        let refused = f.item_url(item, cookie, "inline").await;
        refused.expect(StatusCode::FORBIDDEN, "item.locked");
        assert_eq!(
            refused.json()["reasons"],
            json!([{ "kind": "module_locked" }])
        );
        assert!(!String::from_utf8_lossy(&refused.bytes).contains("/files/"));
    }
    // Staff are not held to the lock (spec 04 B5).
    f.item_url(item, &designer, "inline").await.ok();

    f.clock.set(start() + Duration::from_secs(3_600));
    for cookie in [&student, &observer] {
        let answer = f.item_url(item, cookie, "inline").await;
        let body = answer.ok();
        assert_schema("SignedUrl", &body);
        assert_eq!(body["disposition"], "inline");
        let served = f.fetch(&path_of(&body), &[]).await;
        assert_eq!(
            (served.status, served.bytes.as_slice()),
            (StatusCode::OK, PDF)
        );
    }
    let uri = format!("/api/v1/course-files/{file}/url");
    f.send(
        Method::POST,
        &uri,
        a.id,
        Some(&student),
        Some(json!({ "disposition": "inline" })),
        &[],
    )
    .await
    .expect(StatusCode::FORBIDDEN, "permission.denied");

    // Not a file item: a header.
    let header = items::NewItem {
        kind: items::Kind::Header,
        title: "Week 1".into(),
        ref_id: None,
        url: None,
        position: None,
    };
    let header = items::create(&f.pool, scope(a.id), a.module, &header, start())
        .await
        .unwrap()
        .id;
    f.item_url(header, &f.admin, "inline")
        .await
        .expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    // A page item whose reference happens to name a course file is not a file item either.
    let page = new_id();
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, title)
         VALUES ($1, $2, $3, $4, 9, 'page', $5, 'A page')",
    )
    .bind(page)
    .bind(a.id)
    .bind(a.course)
    .bind(a.module)
    .bind(file)
    .execute(&f.pool)
    .await
    .unwrap();
    let refused = f.item_url(page, &f.admin, "inline").await;
    refused.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(
        refused.json()["errors"],
        json!([{ "field": "", "code": "item.not_a_file" }])
    );
    // Unpublished, the header is not found for the Student, and is not a file for staff.
    f.item_url(header, &student, "inline")
        .await
        .expect(StatusCode::NOT_FOUND, "item.not_found");
}

/// T13 (B16, B17): a token with a changed byte, one opened by a server with another key ring,
/// one fetched 1 s after it expires, one whose course file has been deleted or now names other
/// bytes, and one fetched in another organization all answer 404 `file.not_found` with the same
/// body. Until then the URL
/// serves the file, with `Cache-Control` counting down to the expiry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_every_unusable_token_answers_the_same_404(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let file = f.upload("handout.pdf", PDF).await;
    let path = f.file_path(file, "inline").await;
    let served = f.fetch(&path, &[]).await;
    assert_eq!(served.status, StatusCode::OK, "{served:?}");
    assert_eq!(served.header(header::CACHE_CONTROL), "private, max-age=600");

    let mut refusals = Vec::new();
    // A changed character of the token.
    let token = path.strip_prefix("/files/").unwrap();
    let mut changed = token.as_bytes().to_vec();
    let at = changed.len() / 2;
    changed[at] = if changed[at] == b'A' { b'B' } else { b'A' };
    refusals.push(
        f.fetch(
            &format!("/files/{}", String::from_utf8(changed).unwrap()),
            &[],
        )
        .await,
    );
    refusals.push(f.fetch("/files/not-a-token", &[]).await);
    // Another ring.
    let (other_storage, _dir) = storage(Backend::Local);
    let other = app(&f.pool, &f.clock, Some(other_storage), OTHER_KEYS);
    let request = Request::builder()
        .uri(&path)
        .header(TEST_ORG_HEADER, f.a.id.to_string())
        .body(Body::empty())
        .unwrap();
    refusals.push(answer(&other, request).await);
    // Another organization's request.
    let b = org(&f.pool, "org-b").await;
    refusals.push(f.send(Method::GET, &path, b.id, None, None, &[]).await);
    // 1 s before the expiry it serves; at and after it, it does not.
    f.clock.set(start() + Duration::from_secs(599));
    let last = f.fetch(&path, &[]).await;
    assert_eq!(
        (last.status, last.header(header::CACHE_CONTROL)),
        (StatusCode::OK, "private, max-age=1")
    );
    f.clock.set(start() + Duration::from_secs(601));
    refusals.push(f.fetch(&path, &[]).await);
    // A course file deleted since.
    f.clock.set(start());
    let gone = f.upload("gone.pdf", b"%PDF-1.7 gone\n").await;
    let gone_path = f.file_path(gone, "attachment").await;
    f.send(
        Method::DELETE,
        &format!("/api/v1/course-files/{gone}"),
        f.a.id,
        Some(&f.admin),
        None,
        &[],
    )
    .await;
    refusals.push(f.fetch(&gone_path, &[]).await);
    // A course file that names another stored file than the token's.
    let other = f.upload("other.pdf", b"%PDF-1.7 other\n").await;
    let moved = f.file_path(file, "inline").await;
    sqlx::query(
        "UPDATE course_files SET file_id = (SELECT file_id FROM course_files WHERE id = $2)
         WHERE id = $1",
    )
    .bind(file)
    .bind(other)
    .execute(&f.pool)
    .await
    .unwrap();
    refusals.push(f.fetch(&moved, &[]).await);

    let first = refusals[0].bytes.clone();
    for refused in &refusals {
        refused.expect(StatusCode::NOT_FOUND, "file.not_found");
        assert_eq!(refused.bytes, first, "one answer for every reason");
    }
}

/// T14 (B17): the headers that keep a served file from running.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_a_served_file_carries_headers_that_keep_it_inert(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pdf = f.upload("Handbook.pdf", PDF).await;
    let png = f.upload("Diagram.png", PNG).await;
    let html = f.upload("page.html", HTML).await;
    let svg = f
        .upload("logo.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>")
        .await;
    let named = f.upload("Réglement d'été.pdf", b"%PDF-1.7 accents\n").await;

    let served = f.fetch(&f.file_path(pdf, "inline").await, &[]).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(served.header(header::CONTENT_TYPE), "application/pdf");
    assert_eq!(
        served.header(header::CONTENT_DISPOSITION),
        "inline; filename=\"Handbook.pdf\"; filename*=UTF-8''Handbook.pdf"
    );
    assert_eq!(served.header(header::CONTENT_SECURITY_POLICY), PDF_POLICY);
    assert!(
        !served
            .header(header::CONTENT_SECURITY_POLICY)
            .contains("sandbox")
    );
    let common = [
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::ACCEPT_RANGES, "bytes"),
    ];
    for (name, value) in common.clone() {
        assert_eq!(served.header(name), value);
    }
    assert_eq!(
        served.headers["cross-origin-resource-policy"],
        "same-origin"
    );
    let etag = served.header(header::ETAG).to_owned();
    assert_eq!(etag.len(), 66, "the quoted SHA-256");

    let image = f.fetch(&f.file_path(png, "inline").await, &[]).await;
    assert_eq!(image.header(header::CONTENT_TYPE), "image/png");
    assert!(
        image
            .header(header::CONTENT_DISPOSITION)
            .starts_with("inline;")
    );
    assert_eq!(
        image.header(header::CONTENT_SECURITY_POLICY),
        SANDBOX_POLICY
    );
    // An attachment URL of a PDF is an attachment.
    let download = f.fetch(&f.file_path(pdf, "attachment").await, &[]).await;
    assert!(
        download
            .header(header::CONTENT_DISPOSITION)
            .starts_with("attachment;")
    );

    // Markup asked for inline is an attachment, typed so no browser renders it, and sandboxed.
    for markup in [html, svg] {
        let served = f.fetch(&f.file_path(markup, "inline").await, &[]).await;
        assert_eq!(served.status, StatusCode::OK);
        assert_eq!(
            served.header(header::CONTENT_TYPE),
            "application/octet-stream"
        );
        assert!(
            served
                .header(header::CONTENT_DISPOSITION)
                .starts_with("attachment;")
        );
        assert_eq!(
            served.header(header::CONTENT_SECURITY_POLICY),
            SANDBOX_POLICY
        );
        for (name, value) in common.clone() {
            assert_eq!(served.header(name), value);
        }
    }
    let accented = f.fetch(&f.file_path(named, "attachment").await, &[]).await;
    assert_eq!(
        accented.header(header::CONTENT_DISPOSITION),
        "attachment; filename=\"R_glement d'_t_.pdf\"; \
         filename*=UTF-8''R%C3%A9glement%20d%27%C3%A9t%C3%A9.pdf"
    );

    let path = f.file_path(pdf, "inline").await;
    let not_modified = f
        .fetch(&path, &[(header::IF_NONE_MATCH, etag.as_str())])
        .await;
    assert_eq!(not_modified.status, StatusCode::NOT_MODIFIED);
    assert!(not_modified.bytes.is_empty());
    assert_eq!(not_modified.header(header::ETAG), etag);
}

/// T14 (B17): one range answers 206 with the bytes it names, one past the end 416, on the local
/// store and on MinIO.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_one_range_is_served_from_either_store(pool: PgPool) {
    let bytes: Vec<u8> = (0..1000u32)
        .map(|n| u8::try_from(n % 251).unwrap())
        .collect();
    for backend in [Backend::Local, Backend::Minio] {
        let (store, dir) = storage(backend);
        let f = Fixture::with(pool.clone(), Some(store), dir, KEYS).await;
        let file = f.upload("bytes.bin", &bytes).await;
        let path = f.file_path(file, "attachment").await;

        let part = f.fetch(&path, &[(header::RANGE, "bytes=0-99")]).await;
        assert_eq!(part.status, StatusCode::PARTIAL_CONTENT, "{backend:?}");
        assert_eq!(part.bytes, bytes[..100], "{backend:?}");
        assert_eq!(part.header(header::CONTENT_RANGE), "bytes 0-99/1000");
        assert_eq!(part.header(header::CONTENT_LENGTH), "100");
        let tail = f.fetch(&path, &[(header::RANGE, "bytes=-10")]).await;
        assert_eq!(tail.bytes, bytes[990..], "{backend:?}");
        let refused = f.fetch(&path, &[(header::RANGE, "bytes=999999-")]).await;
        refused.expect(StatusCode::RANGE_NOT_SATISFIABLE, "file.range_invalid");
        assert_eq!(refused.header(header::CONTENT_RANGE), "bytes */1000");
        let whole = f.fetch(&path, &[]).await;
        assert_eq!(
            (whole.status, whole.bytes.len()),
            (StatusCode::OK, 1000),
            "{backend:?}"
        );
    }
}

/// T1a (B1a): without storage both URL routes and `GET /files/<token>` answer 503
/// `storage.not_configured` to a caller they would serve; the URL routes still answer 401
/// first.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1a_without_storage_the_delivery_routes_answer_503(pool: PgPool) {
    let f = Fixture::with(pool, None, None, KEYS).await;
    let a = f.a;
    // A course file and item, as an instance with storage once made them.
    let file_id = new_id();
    sqlx::query(
        "INSERT INTO files (id, org_id, sha256, size, mime, storage_key)
         VALUES ($1, $2, sha256('x'::bytea), 1, 'application/pdf', 'k')",
    )
    .bind(file_id)
    .bind(a.id)
    .execute(&f.pool)
    .await
    .unwrap();
    let course_file = new_id();
    sqlx::query(
        "INSERT INTO course_files (id, org_id, course_id, file_id, folder, name)
         VALUES ($1, $2, $3, $4, '/', 'x.pdf')",
    )
    .bind(course_file)
    .bind(a.id)
    .bind(a.course)
    .bind(file_id)
    .execute(&f.pool)
    .await
    .unwrap();
    let item = f.file_item(a, course_file).await;
    let body = json!({ "disposition": "inline" });
    for uri in [
        format!("/api/v1/items/{item}/file-url"),
        format!("/api/v1/course-files/{course_file}/url"),
    ] {
        f.send(Method::POST, &uri, a.id, None, Some(body.clone()), &[])
            .await
            .expect(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
        let answer = f
            .send(
                Method::POST,
                &uri,
                a.id,
                Some(&f.admin),
                Some(body.clone()),
                &[],
            )
            .await;
        answer.expect(StatusCode::SERVICE_UNAVAILABLE, "storage.not_configured");
        assert!(
            answer.json()["detail"]
                .as_str()
                .unwrap()
                .contains("STEPCLIMB_STORAGE_URL")
        );
    }
    f.fetch("/files/anything", &[])
        .await
        .expect(StatusCode::SERVICE_UNAVAILABLE, "storage.not_configured");
}

/// T19: org A's admin naming org B's course file or file item gets 404, and a URL org B issued
/// serves only org B's file and only in org B.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t19_another_organizations_files_are_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let b = org(&f.pool, "org-b").await;
    let b_admin = f.holder(b.id, "org_admin", Scope::Organization).await;
    let theirs = f.upload_to(b, &b_admin, "theirs.pdf", PDF).await;
    let item = f.file_item(b, theirs).await;
    let body = json!({ "disposition": "inline" });
    let calls = [
        (format!("/api/v1/items/{item}/file-url"), "item.not_found"),
        (
            format!("/api/v1/course-files/{theirs}/url"),
            "file.not_found",
        ),
    ];
    for (uri, code) in calls {
        let answer = f
            .send(
                Method::POST,
                &uri,
                f.a.id,
                Some(&f.admin),
                Some(body.clone()),
                &[],
            )
            .await;
        answer.expect(StatusCode::NOT_FOUND, code);
        assert!(!String::from_utf8_lossy(&answer.bytes).contains("/files/"));
    }
    let uri = format!("/api/v1/course-files/{theirs}/url");
    let issued = f
        .send(Method::POST, &uri, b.id, Some(&b_admin), Some(body), &[])
        .await;
    let path = path_of(&issued.ok());
    f.fetch(&path, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    let served = f.send(Method::GET, &path, b.id, None, None, &[]).await;
    assert_eq!(
        (served.status, served.bytes.as_slice()),
        (StatusCode::OK, PDF)
    );
}

/// B16: a URL's answer and its token agree on the expiry, 10 minutes after issue, and the answer
/// names the file it serves.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b16_a_url_expires_ten_minutes_after_issue(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let file = f.upload("handout.pdf", PDF).await;
    let uri = format!("/api/v1/course-files/{file}/url");
    let answer = f
        .send(
            Method::POST,
            &uri,
            f.a.id,
            Some(&f.admin),
            Some(json!({ "disposition": "attachment" })),
            &[],
        )
        .await;
    let body = answer.ok();
    assert_schema("SignedUrl", &body);
    assert_eq!(body["expires_at"], "2025-11-13T02:23:20.000000Z");
    assert_eq!(body["disposition"], "attachment");
    assert_eq!(
        (&body["name"], &body["mime"], &body["size"]),
        (
            &json!("handout.pdf"),
            &json!("application/pdf"),
            &json!(PDF.len())
        )
    );
    assert!(
        body["url"]
            .as_str()
            .unwrap()
            .starts_with("https://lms.example.org/files/")
    );
}

/// B14 with spec 04 B9: in a sequential module, a learner's own progress decides. The file
/// after a required link is locked until the learner views the link; an Observer, who has no
/// progress, stays held.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b14_a_learners_own_progress_opens_a_sequential_file_item(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let link = items::NewItem {
        kind: items::Kind::ExternalLink,
        title: "Read first".into(),
        ref_id: None,
        url: Some("https://example.com/first".into()),
        position: None,
    };
    let link = items::create(&f.pool, scope(a.id), a.module, &link, start())
        .await
        .unwrap()
        .id;
    let publish = items::ItemPatch {
        published: Some(true),
        ..Default::default()
    };
    items::update(&f.pool, scope(a.id), link, &publish, start())
        .await
        .unwrap();
    items::set_requirement(
        &f.pool,
        scope(a.id),
        link,
        Some(stepclimb_core::rules::Requirement::View),
        start(),
    )
    .await
    .unwrap();
    let item = f.file_item(a, f.upload("handout.pdf", PDF).await).await;
    let sequential = modules::RulesPatch {
        require_sequential: Some(true),
        ..Default::default()
    };
    modules::set_rules(&f.pool, scope(a.id), a.module, &sequential, start())
        .await
        .unwrap();
    let (learner, student) = f.enrolled(a, "student", Vec::new()).await;
    let (_, observer) = f.enrolled(a, "observer", vec![learner]).await;

    f.item_url(item, &student, "inline")
        .await
        .expect(StatusCode::FORBIDDEN, "item.locked");
    let step = json!({ "action": "view" });
    let uri = format!("/api/v1/items/{link}/progress");
    let viewed = f
        .send(Method::POST, &uri, a.id, Some(&student), Some(step), &[])
        .await;
    assert_eq!(viewed.status, StatusCode::OK, "{viewed:?}");
    f.item_url(item, &student, "inline").await.ok();
    f.item_url(item, &observer, "inline")
        .await
        .expect(StatusCode::FORBIDDEN, "item.locked");
}

/// The headers the end-to-end preview test serves its synthetic PDF and image with
/// (`web/e2e/fixtures/signed-file-headers.json`, read by `web/e2e/file-item.spec.ts`) are the ones
/// this server sends for the same bytes and names, so what the browsers are checked against is
/// what they get (Q8).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_e2e_preview_serves_what_the_server_sends(pool: PgPool) {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/e2e/fixtures");
    let read = |name: &str| std::fs::read(fixtures.join(name)).expect("a fixture");
    let expected: serde_json::Map<String, Value> =
        serde_json::from_slice(&read("signed-file-headers.json")).expect("the headers fixture");
    let f = Fixture::new(pool).await;
    let pdf = f
        .upload("Ground school handbook.pdf", &read("handbook.pdf"))
        .await;
    let png = f.upload("Airport diagram.png", &read("diagram.png")).await;
    let cases = [
        ("pdf-inline", pdf, "inline"),
        ("pdf-attachment", pdf, "attachment"),
        ("png-inline", png, "inline"),
    ];
    assert_eq!(expected.len(), cases.len());
    for (key, file, disposition) in cases {
        let served = f.fetch(&f.file_path(file, disposition).await, &[]).await;
        assert_eq!(served.status, StatusCode::OK, "{key}");
        let headers = expected[key].as_object().expect("a header map");
        for (name, value) in headers {
            let name = header::HeaderName::from_bytes(name.as_bytes()).expect("a header name");
            assert_eq!(served.header(name.clone()), value, "{key}: {name}");
        }
    }
}

/// A `file` item whose course file is in another course is never served: `items::create` keeps
/// it out, and the URL route checks again (written here by SQL, as no route writes one).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_item_naming_another_courses_file_serves_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let item = f.file_item(a, f.upload("handout.pdf", PDF).await).await;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
            .bind(a.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let new = courses::NewCourse {
        account_id: root,
        title: "Course S2".into(),
        code: "S2".into(),
    };
    let elsewhere = courses::create(&f.pool, scope(a.id), &new, start(), Actor::Instance)
        .await
        .unwrap()
        .id;
    let theirs = f
        .upload_to(
            Org {
                course: elsewhere,
                ..a
            },
            &f.admin,
            "theirs.pdf",
            b"%PDF-1.7 theirs\n",
        )
        .await;
    sqlx::query("UPDATE module_items SET ref_id = $2 WHERE id = $1")
        .bind(item)
        .bind(theirs)
        .execute(&f.pool)
        .await
        .unwrap();
    let refused = f.item_url(item, &f.admin, "inline").await;
    refused.expect(StatusCode::NOT_FOUND, "file.not_found");
    assert!(!String::from_utf8_lossy(&refused.bytes).contains("/files/"));
}

/// `HEAD` answers the headers a `GET` would, without reading the store: here the object is gone,
/// which a `GET` cannot serve.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn head_answers_the_headers_without_reading_the_store(pool: PgPool) {
    let (store, dir) = storage(Backend::Local);
    let f = Fixture::with(pool, Some(store.clone()), dir, KEYS).await;
    let file = f.upload("Handbook.pdf", PDF).await;
    let path = f.file_path(file, "inline").await;
    let got = f.fetch(&path, &[]).await;
    let head = f.send(Method::HEAD, &path, f.a.id, None, None, &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.bytes.is_empty());
    assert_eq!(head.headers, got.headers);

    let mut hasher = stepclimb_core::files::Sha256Hasher::new();
    hasher.update(PDF);
    let key = store.file_path(scope(f.a.id), hasher.finish().sha256());
    store.store().delete(&key).await.expect("delete the object");
    let head = f.send(Method::HEAD, &path, f.a.id, None, None, &[]).await;
    assert_eq!((head.status, &head.headers), (StatusCode::OK, &got.headers));
    let part = f
        .send(
            Method::HEAD,
            &path,
            f.a.id,
            None,
            None,
            &[(header::RANGE, "bytes=0-9")],
        )
        .await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.header(header::CONTENT_LENGTH), "10");
    let gone = f.fetch(&path, &[]).await;
    assert_eq!(
        gone.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "a GET reads the store"
    );
}
