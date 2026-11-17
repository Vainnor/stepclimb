// SPDX-License-Identifier: AGPL-3.0-or-later
//! Video uploads over tus and the videos of a course, over the router
//! (docs/specs/m1/08-uploaded-video.md B1 to B7, B21; T1 to T3 and T16 for these routes). The
//! permission suite's generated cases for each route (T15, T16) are in `tests/permissions.rs`.
//!
//! The upload tests run against the MinIO in the gate's own Compose project (`SC_CI_MINIO_URL`,
//! as `tests/storage.rs` describes), each under a prefix of its own, and against a temporary
//! local directory. No case reaches the network outside the stack. Needs `DATABASE_URL`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use axum::{
    Extension, Router,
    body::{Body, Bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::{BodyExt, Channel};
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
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
    videos::{self, Terminating},
};
use stepclimb_server::{
    AppState,
    config::{S3Config, StorageConfig},
    request_org::TEST_ORG_HEADER,
    router, session,
    storage::Storage,
    videos_cleanup,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const HOUR: Duration = Duration::from_secs(60 * 60);

const MIB: u64 = 1024 * 1024;

/// The largest video these tests accept: 8 MiB, room for chunks of the 1 MiB minimum.
const MAX_VIDEO_BYTES: u64 = 8 * MIB;

/// The first bytes of an MP4 file: an `ftyp` box with the `isom` brand.
const MP4_HEAD: &[u8] = b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2";

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0";

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

/// The store of `backend`, under a place of the test's own.
fn storage(backend: Backend) -> (Storage, Option<TempDir>) {
    let (config, dir) = match backend {
        Backend::Local => {
            let dir = std::env::temp_dir().join(format!("stepclimb-videos-{}", new_id()));
            std::fs::create_dir_all(&dir).expect("create the directory");
            let config = StorageConfig::Local { root: dir.clone() };
            (config, Some(TempDir(dir)))
        }
        Backend::Minio => {
            let endpoint = std::env::var("SC_CI_MINIO_URL").expect(
                "SC_CI_MINIO_URL names the gate's MinIO: run these tests inside `just ci` or \
                 `python3 bin/ci_gate.py run -- cargo nextest run -p stepclimb-server --test videos`",
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
        .expect("storage is configured")
        .with_max_video_bytes(MAX_VIDEO_BYTES);
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
    fn ok(&self, status: StatusCode) -> &Value {
        assert_eq!(self.status, status, "{self:?}");
        &self.body
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    fn offset(&self) -> u64 {
        self.header("upload-offset")
            .expect("Upload-Offset")
            .parse()
            .expect("a number")
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

    async fn with(pool: PgPool, storage: Option<Storage>, dir: Option<TempDir>) -> Self {
        roles::sync_builtin(&pool, SystemTime::now())
            .await
            .expect("sync roles");
        let clock = ManualClock::new(SystemTime::now());
        let app = app(&pool, &clock, storage.clone());
        let slug = format!("org-a-{}", &new_id().simple().to_string()[24..]);
        let a = tree(&pool, &slug, clock.clock().now()).await;
        let mut fixture = Self {
            pool,
            app,
            clock,
            storage: storage.unwrap_or_else(|| self::storage(Backend::Local).0),
            _dir: dir,
            a,
            admin: String::new(),
        };
        fixture.admin = fixture.admin_of(a.org).await;
        fixture
    }

    fn now(&self) -> SystemTime {
        self.clock.clock().now()
    }

    /// A second server of the same instance: its own router on the same database and store.
    fn second_server(&self) -> Router {
        app(&self.pool, &self.clock, Some(self.storage.clone()))
    }

    async fn admin_of(&self, org: Uuid) -> String {
        let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind("org_admin")
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role");
        self.holder_of(org, role).await
    }

    /// A user of org A holding a custom role with only `permissions` at the organization: their
    /// session cookie.
    async fn holder(&self, permissions: &[&str]) -> String {
        self.holder_at(permissions, Scope::Organization).await
    }

    /// A user of org A holding a custom role with only `permissions` at `at`: their cookie.
    async fn holder_at(&self, permissions: &[&str], at: Scope) -> String {
        let new = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Video role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role = roles::create_custom(
            &self.pool,
            scope(self.a.org),
            &new,
            self.now(),
            Actor::Instance,
        )
        .await
        .expect("create role")
        .id;
        let user = self.user(self.a.org).await;
        self.assign(self.a.org, user, role, at).await;
        self.session(self.a.org, user).await
    }

    async fn holder_of(&self, org: Uuid, role: Uuid) -> String {
        let user = self.user(org).await;
        self.assign(org, user, role, Scope::Organization).await;
        self.session(org, user).await
    }

    async fn user(&self, org: Uuid) -> Uuid {
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
        users::create(&self.pool, scope(org), &new_user, self.now())
            .await
            .expect("create user")
            .id
    }

    async fn assign(&self, org: Uuid, user: Uuid, role: Uuid, at: Scope) {
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
    }

    async fn session(&self, org: Uuid, user: Uuid) -> String {
        let started = session::start(&self.pool, scope(org), user, &HeaderMap::new(), self.now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    fn base(&self, method: Method, uri: &str, org: Uuid) -> axum::http::request::Builder {
        let cookie = if org == self.a.org {
            self.admin.clone()
        } else {
            String::new()
        };
        Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie)
            .header("tus-resumable", "1.0.0")
    }

    /// Org A's admin starts an upload of `length` bytes titled `title` into `course`.
    async fn create(&self, course: Uuid, length: u64, title: &str) -> Answer {
        let request = self
            .base(
                Method::POST,
                &format!("/api/v1/courses/{course}/video-uploads"),
                self.a.org,
            )
            .header("upload-length", length)
            .header(
                "upload-metadata",
                format!("title {},filename eC5tcDQ=", STANDARD.encode(title)),
            )
            .header(header::CONTENT_LENGTH, 0)
            .body(Body::empty())
            .expect("a request");
        answer(&self.app, request).await
    }

    /// The created upload's path.
    async fn started(&self, course: Uuid, length: u64) -> String {
        let created = self.create(course, length, "Week 1 lecture").await;
        created.ok(StatusCode::CREATED);
        created.header("location").expect("Location").to_owned()
    }

    async fn patch_on(&self, app: &Router, path: &str, offset: u64, bytes: &[u8]) -> Answer {
        let request = self
            .base(Method::PATCH, path, self.a.org)
            .header(header::CONTENT_TYPE, "application/offset+octet-stream")
            .header("upload-offset", offset)
            .header(header::CONTENT_LENGTH, bytes.len())
            .body(Body::from(bytes.to_vec()))
            .expect("a request");
        answer(app, request).await
    }

    async fn patch(&self, path: &str, offset: u64, bytes: &[u8]) -> Answer {
        self.patch_on(&self.app, path, offset, bytes).await
    }

    async fn send(&self, method: Method, path: &str, body: Option<Value>) -> Answer {
        let mut request = self.base(method, path, self.a.org);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let request = request.body(Body::from(body)).expect("a request");
        answer(&self.app, request).await
    }

    /// The video's row: status and error code.
    async fn video_status(&self, video: Uuid) -> Option<(String, Option<String>)> {
        sqlx::query_as("SELECT status, error_code FROM videos WHERE id = $1")
            .bind(video)
            .fetch_optional(&self.pool)
            .await
            .expect("read the video")
    }

    /// The video of the upload at `path`.
    async fn video_of(&self, path: &str) -> Uuid {
        sqlx::query_scalar("SELECT video_id FROM video_uploads WHERE id = $1")
            .bind(upload_id(path))
            .fetch_one(&self.pool)
            .await
            .expect("the upload's video")
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

    async fn read_object(&self, key: &str) -> Vec<u8> {
        let got = self
            .storage
            .store()
            .get(&self.storage.path(key))
            .await
            .expect("get");
        got.bytes().await.expect("bytes").to_vec()
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
}

fn app(pool: &PgPool, clock: &ManualClock, storage: Option<Storage>) -> Router {
    let state = AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        passwords: Default::default(),
        storage,
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    };
    router(state).layer(Extension(ConnectInfo(CLIENT)))
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

/// Put `org` on a test plan whose `video_storage` limit is `bytes`, by SQL (M1 has no API for
/// it).
async fn limit_video_storage(pool: &PgPool, org: Uuid, bytes: i64) {
    let plan = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, limits)
         VALUES ($1, $2, jsonb_build_object('video_storage', $3::bigint))",
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

/// `n` bytes of an MP4 file: its signature, then filler.
fn mp4(n: usize) -> Vec<u8> {
    let mut bytes = MP4_HEAD.to_vec();
    bytes.resize(n, b'v');
    bytes
}

fn to_usize(n: u64) -> usize {
    usize::try_from(n).expect("a test size fits a usize")
}

fn upload_id(path: &str) -> Uuid {
    let id = path.rsplit('/').next().expect("a segment");
    id.parse().expect("a UUID")
}

/// B4: a creation makes the `uploading` video, attached to the course, and its upload, and
/// answers the upload's `Location` and `Tus-Resumable`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b4_a_creation_makes_the_video_and_its_upload(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let created = f.create(t.course, 30, "  Week 1 lecture ").await;
    let body = created.ok(StatusCode::CREATED).clone();
    assert_schema("VideoUploadCreated", &body);
    let upload = body["upload_id"].as_str().unwrap();
    assert_eq!(
        created.header("location"),
        Some(format!("/api/v1/video-uploads/{upload}").as_str())
    );
    assert_eq!(created.header("tus-resumable"), Some("1.0.0"));
    assert_eq!(body["video"]["title"], "Week 1 lecture");
    assert_eq!(body["video"]["status"], "uploading");
    assert_eq!(body["video"]["source"], "upload");
    let video: Uuid = body["video"]["id"].as_str().unwrap().parse().unwrap();
    let attached: Vec<Uuid> =
        sqlx::query_scalar("SELECT course_id FROM course_videos WHERE video_id = $1")
            .bind(video)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(attached, [t.course]);
    let head = f
        .send(
            Method::HEAD,
            &format!("/api/v1/video-uploads/{upload}"),
            None,
        )
        .await;
    head.ok(StatusCode::OK);
    assert_eq!(
        (head.offset(), head.header("upload-length")),
        (0, Some("30"))
    );
    assert_eq!(head.header("tus-resumable"), Some("1.0.0"));
    // Without a title, the file name is the title.
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/video-uploads", t.course),
            t.org,
        )
        .header("upload-length", 30)
        .header("upload-metadata", "filename bGVjdHVyZS5tcDQ=")
        .body(Body::empty())
        .unwrap();
    let named = answer(&f.app, request).await;
    assert_eq!(
        named.ok(StatusCode::CREATED)["video"]["title"],
        "lecture.mp4"
    );
}

/// T1 (B4): the refusals before anything is created.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_creation_is_refused_before_anything_is_created(pool: PgPool) {
    let f = Fixture::new(pool.clone(), Backend::Local).await;
    let t = f.a;
    f.create(t.course, MAX_VIDEO_BYTES + 1, "Too long")
        .await
        .expect(StatusCode::PAYLOAD_TOO_LARGE, "file.too_large");
    limit_video_storage(&f.pool, t.org, 1000).await;
    f.create(t.course, 600, "Fits")
        .await
        .ok(StatusCode::CREATED);
    let full = f.create(t.course, 401, "Over").await;
    full.expect(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(
        (full.body["limit"].as_str(), full.body["max"].as_u64()),
        (Some("video_storage"), Some(1000))
    );
    f.create(t.course, 400, "Exactly the rest")
        .await
        .ok(StatusCode::CREATED);
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/video-uploads", t.course),
            t.org,
        )
        .header("upload-length", 10)
        .body(Body::empty())
        .unwrap();
    let untitled = answer(&f.app, request).await;
    untitled.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    let request = f
        .base(
            Method::POST,
            &format!("/api/v1/courses/{}/video-uploads", t.course),
            t.org,
        )
        .header("upload-metadata", "title eA==")
        .body(Body::empty())
        .unwrap();
    answer(&f.app, request)
        .await
        .expect(StatusCode::BAD_REQUEST, "request.malformed");
    let request = Request::post(format!("/api/v1/courses/{}/video-uploads", t.course))
        .header(TEST_ORG_HEADER, t.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, f.admin.clone())
        .header("upload-length", 10)
        .body(Body::empty())
        .unwrap();
    let old_client = answer(&f.app, request).await;
    old_client.expect(StatusCode::PRECONDITION_FAILED, "tus.version_unsupported");
    assert_eq!(old_client.header("tus-version"), Some("1.0.0"));
    assert_eq!(old_client.header("tus-resumable"), Some("1.0.0"));
    assert_eq!(f.count("videos", t.org).await, 2);
    assert_eq!(f.count("video_uploads", t.org).await, 2);

    // Without storage: 503, after the 401 an anonymous caller gets.
    let none = Fixture::with(pool, None, None).await;
    let refused = none.create(none.a.course, 10, "No store").await;
    refused.expect(StatusCode::SERVICE_UNAVAILABLE, "storage.not_configured");
    assert_eq!(none.count("videos", none.a.org).await, 0);
}

/// B4, spec 02 B5: an archived course takes no upload, and no chunk of one started before.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_takes_no_upload_or_chunk(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 20).await;
    sqlx::query("UPDATE courses SET status = 'archived' WHERE id = $1")
        .bind(t.course)
        .execute(&f.pool)
        .await
        .unwrap();
    f.create(t.course, 20, "Late")
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    f.patch(&path, 0, &mp4(20))
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    assert_eq!(f.count("videos", t.org).await, 1);
    assert!(f.objects(t.org).await.is_empty());
}

/// T2 (B5, B6, B7): an upload in three chunks, the second sent to a second server, reaches its
/// length; a stale offset answers 409; after a dropped connection `HEAD` reports the stored
/// offset and the upload resumes from it. The complete upload's video is `processing` with a
/// transcode job on the `transcode` queue.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_an_upload_resumes_across_servers_and_dropped_connections(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        let length = 2 * MIB + 10;
        let bytes = mp4(usize::try_from(length).unwrap());
        let path = f.started(t.course, length).await;
        let (one, two) = (to_usize(MIB), to_usize(2 * MIB));
        let first = f.patch(&path, 0, &bytes[..one]).await;
        first.ok(StatusCode::NO_CONTENT);
        assert_eq!(first.offset(), MIB);
        assert_eq!(first.header("tus-resumable"), Some("1.0.0"));
        let second = f.second_server();
        let on_second = f.patch_on(&second, &path, MIB, &bytes[one..two]).await;
        assert_eq!(on_second.offset(), 2 * MIB, "{backend:?}");
        f.patch(&path, MIB, &bytes[one..two])
            .await
            .expect(StatusCode::CONFLICT, "video.upload_offset_mismatch");
        // A connection that drops after 3 bytes of the last chunk stores none of it.
        let (mut sender, body) = Channel::<Bytes, std::io::Error>::new(1);
        let request = f
            .base(Method::PATCH, &path, t.org)
            .header(header::CONTENT_TYPE, "application/offset+octet-stream")
            .header("upload-offset", 2 * MIB)
            .header(header::CONTENT_LENGTH, 10)
            .body(Body::new(body))
            .unwrap();
        let feeding = tokio::spawn(async move {
            sender.send_data(Bytes::from_static(b"vvv")).await.unwrap();
            sender.abort(std::io::Error::other("connection reset"));
        });
        let dropped = answer(&f.app, request).await;
        feeding.await.unwrap();
        dropped.expect(StatusCode::BAD_REQUEST, "request.malformed");
        let head = f.send(Method::HEAD, &path, None).await;
        assert_eq!(head.offset(), 2 * MIB, "{backend:?}");
        let last = f
            .patch_on(&second, &path, head.offset(), &bytes[two..])
            .await;
        assert_eq!(last.offset(), length);
        let video = f.video_of(&path).await;
        let prefix = format!("orgs/{}/videos/{video}/upload", t.org);
        let offsets = [0, MIB, 2 * MIB];
        let mut expected: Vec<String> = offsets
            .iter()
            .map(|offset| format!("{prefix}/{offset}"))
            .collect();
        expected.sort();
        assert_eq!(f.objects(t.org).await, expected, "{backend:?}");
        let mut stored = Vec::new();
        for offset in offsets {
            stored.extend(f.read_object(&format!("{prefix}/{offset}")).await);
        }
        assert_eq!(stored, bytes);
        assert_eq!(
            f.video_status(video).await,
            Some(("processing".to_owned(), None))
        );
        let jobs: Vec<(String, String, String)> =
            sqlx::query_as("SELECT kind, queue, payload->>'video_id' FROM jobs WHERE org_id = $1")
                .bind(t.org)
                .fetch_all(&f.pool)
                .await
                .unwrap();
        assert_eq!(
            jobs,
            [(
                "video.transcode".to_owned(),
                "transcode".to_owned(),
                video.to_string()
            )]
        );
        let done = f.send(Method::HEAD, &path, None).await;
        assert_eq!(done.offset(), length);
    }
}

/// B6, D5: two chunks sent at once at the same offset: one is stored and answers 204, the other
/// answers 409, and exactly one part is left.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_chunks_at_one_offset_store_one_part(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 3 * MIB).await;
    let chunk = mp4(to_usize(MIB));
    let other = vec![b'x'; to_usize(MIB)];
    let (a, b) = tokio::join!(f.patch(&path, 0, &chunk), f.patch(&path, 0, &other));
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::NO_CONTENT, StatusCode::CONFLICT]);
    let refused = if a.status == StatusCode::CONFLICT {
        &a
    } else {
        &b
    };
    assert_eq!(refused.code(), "video.upload_offset_mismatch");
    let video = f.video_of(&path).await;
    let part = format!("orgs/{}/videos/{video}/upload/0", t.org);
    assert_eq!(f.objects(t.org).await, std::slice::from_ref(&part));
    let stored = f.read_object(&part).await;
    let winner = if a.status == StatusCode::NO_CONTENT {
        &chunk
    } else {
        &other
    };
    assert_eq!(&stored, winner);
    assert_eq!(f.send(Method::HEAD, &path, None).await.offset(), MIB);
}

/// B5, B6: a chunk with the wrong type, one that runs past the length, a chunk under 1 MiB that
/// does not complete the upload (D15), and an empty chunk.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_chunk_is_refused_unless_it_is_an_offset_stream_within_the_length(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 10).await;
    let request = f
        .base(Method::PATCH, &path, t.org)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header("upload-offset", 0)
        .header(header::CONTENT_LENGTH, 4)
        .body(Body::from("abcd"))
        .unwrap();
    answer(&f.app, request).await.expect(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "request.unsupported_media_type",
    );
    f.patch(&path, 0, &mp4(11))
        .await
        .expect(StatusCode::PAYLOAD_TOO_LARGE, "file.too_large");
    let empty = f.patch(&path, 0, b"").await;
    empty.ok(StatusCode::NO_CONTENT);
    assert_eq!(empty.offset(), 0);
    assert!(f.objects(t.org).await.is_empty());

    let long = f.started(t.course, 2 * MIB + 1).await;
    f.patch(&long, 0, &mp4(to_usize(MIB) - 1))
        .await
        .expect(StatusCode::BAD_REQUEST, "video.chunk_too_small");
    assert!(f.objects(t.org).await.is_empty());
    let at_minimum = f.patch(&long, 0, &mp4(to_usize(MIB))).await;
    assert_eq!(at_minimum.offset(), MIB);
    let rest = f.patch(&long, MIB, &vec![b'v'; to_usize(MIB) + 1]).await;
    assert_eq!(rest.offset(), 2 * MIB + 1, "a last chunk may be any size");
}

/// B7, D7: a complete upload whose first bytes are no video fails with `video.not_a_video`; its
/// parts and upload row are deleted, the failed video stays, and nothing is enqueued.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b7_a_complete_upload_that_is_not_a_video_fails(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let t = f.a;
    let length = u64::try_from(PNG.len()).unwrap();
    let path = f.started(t.course, length).await;
    let last = f.patch(&path, 0, PNG).await;
    last.ok(StatusCode::NO_CONTENT);
    assert_eq!(last.offset(), length);
    let video: Uuid = sqlx::query_scalar("SELECT id FROM videos WHERE org_id = $1")
        .bind(t.org)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.video_status(video).await,
        Some(("failed".to_owned(), Some("video.not_a_video".to_owned())))
    );
    assert_eq!(f.count("video_uploads", t.org).await, 0);
    assert_eq!(f.count("jobs", t.org).await, 0);
    assert!(f.objects(t.org).await.is_empty());
    f.send(Method::HEAD, &path, None)
        .await
        .ok(StatusCode::NOT_FOUND);
    let read = f
        .send(Method::GET, &format!("/api/v1/videos/{video}"), None)
        .await;
    assert_eq!(read.ok(StatusCode::OK)["error_code"], "video.not_a_video");
}

/// T3 (B6): an incomplete upload untouched for 24 hours is deleted with its parts and video by
/// `videos.cleanup`; one touched 23 hours ago is kept, and so is a complete one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_cleanup_deletes_uploads_untouched_for_a_day(pool: PgPool) {
    for backend in [Backend::Minio, Backend::Local] {
        sqlx::query("DELETE FROM jobs")
            .execute(&pool)
            .await
            .unwrap();
        let f = Fixture::new(pool.clone(), backend).await;
        let t = f.a;
        let chunk = mp4(to_usize(MIB));
        let stale = f.started(t.course, 2 * MIB).await;
        f.patch(&stale, 0, &chunk).await.ok(StatusCode::NO_CONTENT);
        let complete = f.started(t.course, 30).await;
        f.patch(&complete, 0, &mp4(30))
            .await
            .ok(StatusCode::NO_CONTENT);
        f.clock.advance(HOUR);
        let recent = f.started(t.course, 2 * MIB).await;
        f.patch(&recent, 0, &chunk).await.ok(StatusCode::NO_CONTENT);
        // 24 hours and a minute after the stale upload's chunk, 23 after the recent one's.
        let at = f.now() + 23 * HOUR + Duration::from_secs(60);
        let report = videos_cleanup::run(&f.pool, &f.storage, at).await.unwrap();
        assert_eq!(
            report,
            videos_cleanup::Report {
                stale_marked: 1,
                not_a_video_marked: 0,
                finished: 1,
                failed: 0,
            },
            "{backend:?}"
        );
        f.send(Method::HEAD, &stale, None)
            .await
            .ok(StatusCode::NOT_FOUND);
        assert_eq!(f.send(Method::HEAD, &recent, None).await.offset(), MIB);
        assert_eq!(f.send(Method::HEAD, &complete, None).await.offset(), 30);
        assert_eq!(f.count("videos", t.org).await, 2);
        let parts: Vec<String> = f.objects(t.org).await;
        assert_eq!(parts.len(), 2, "{backend:?}: {parts:?}");
        let again = videos_cleanup::run(&f.pool, &f.storage, at).await.unwrap();
        assert_eq!(again, videos_cleanup::Report::default());
    }
}

/// D7: the parts of a video that failed as not a video, whose immediate delete did not happen,
/// are deleted by `videos.cleanup` at any age, and the failed video stays. The original of a
/// video that failed for another reason (`video.transcode_failed`) is kept for a retry (B2).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_deletes_only_the_parts_of_a_video_that_is_not_a_video(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let chunk = mp4(to_usize(MIB));
    let not_a_video = f.started(t.course, 2 * MIB).await;
    f.patch(&not_a_video, 0, &chunk)
        .await
        .ok(StatusCode::NO_CONTENT);
    let transcode_failed = f.started(t.course, 2 * MIB).await;
    f.patch(&transcode_failed, 0, &chunk)
        .await
        .ok(StatusCode::NO_CONTENT);
    for (path, code) in [
        (&not_a_video, "video.not_a_video"),
        (&transcode_failed, "video.transcode_failed"),
    ] {
        sqlx::query("UPDATE videos SET status = 'failed', error_code = $2 WHERE id = $1")
            .bind(f.video_of(path).await)
            .bind(code)
            .execute(&f.pool)
            .await
            .unwrap();
    }
    let kept = f.video_of(&transcode_failed).await;
    let report = videos_cleanup::run(&f.pool, &f.storage, f.now())
        .await
        .unwrap();
    assert_eq!(
        (report.not_a_video_marked, report.finished, report.failed),
        (1, 1, 0)
    );
    assert_eq!(
        f.objects(t.org).await,
        [format!("orgs/{}/videos/{kept}/upload/0", t.org)]
    );
    let uploads: Vec<Uuid> =
        sqlx::query_scalar("SELECT video_id FROM video_uploads WHERE org_id = $1")
            .bind(t.org)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(uploads, [kept]);
    assert_eq!(f.count("videos", t.org).await, 2);
}

/// D13: a termination whose part delete did not finish is finished by `videos.cleanup`, and no
/// chunk is taken for a terminating upload meanwhile.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_finishes_a_termination_and_a_marked_upload_takes_no_chunk(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 2 * MIB).await;
    f.patch(&path, 0, &mp4(to_usize(MIB)))
        .await
        .ok(StatusCode::NO_CONTENT);
    // As a termination leaves an upload when the server stops after its commit.
    sqlx::query("UPDATE video_uploads SET terminating = 'video' WHERE org_id = $1")
        .bind(t.org)
        .execute(&f.pool)
        .await
        .unwrap();
    f.patch(&path, MIB, &mp4(to_usize(MIB)))
        .await
        .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
    f.send(Method::HEAD, &path, None)
        .await
        .ok(StatusCode::NOT_FOUND);
    f.send(Method::DELETE, &path, None)
        .await
        .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
    assert_eq!(f.objects(t.org).await.len(), 1);
    let report = videos_cleanup::run(&f.pool, &f.storage, f.now())
        .await
        .unwrap();
    assert_eq!((report.finished, report.failed), (1, 0));
    assert!(f.objects(t.org).await.is_empty());
    assert_eq!(f.count("videos", t.org).await, 0);
}

/// D13: a chunk that passed its first look at the upload and then waits for the upload row's
/// lock, while a termination marks the upload and commits, answers 404 once it gets the lock and
/// writes no part: the check under the lock, not the first look, is what stops it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_chunk_waiting_on_a_termination_writes_no_part(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 2 * MIB).await;
    let id = upload_id(&path);
    let mut holder = f.pool.begin().await.unwrap();
    videos::lock_upload(&mut holder, scope(t.org), id)
        .await
        .unwrap()
        .expect("the upload");
    let request = f
        .base(Method::PATCH, &path, t.org)
        .header(header::CONTENT_TYPE, "application/offset+octet-stream")
        .header("upload-offset", 0)
        .header(header::CONTENT_LENGTH, MIB)
        .body(Body::from(mp4(to_usize(MIB))))
        .unwrap();
    let app = f.app.clone();
    let chunk = tokio::spawn(async move { answer(&app, request).await });
    // Wait until the chunk's transaction waits for the row lock the holder has.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        assert!(!chunk.is_finished(), "the chunk did not wait for the lock");
        assert!(
            std::time::Instant::now() < deadline,
            "the chunk never waited"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    videos::mark_terminating(&mut holder, scope(t.org), id, Terminating::Video, f.now())
        .await
        .unwrap();
    holder.commit().await.unwrap();
    chunk
        .await
        .unwrap()
        .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
    assert!(
        f.objects(t.org).await.is_empty(),
        "no part, and no temporary object"
    );
}

/// B5: ending an upload deletes its parts and its video; a complete upload whose video is
/// processing cannot be ended.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn termination_deletes_the_upload_its_parts_and_its_video(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Minio).await;
    let t = f.a;
    let path = f.started(t.course, 3 * MIB).await;
    f.patch(&path, 0, &mp4(to_usize(MIB)))
        .await
        .ok(StatusCode::NO_CONTENT);
    f.patch(&path, MIB, &mp4(to_usize(MIB)))
        .await
        .ok(StatusCode::NO_CONTENT);
    let ended = f.send(Method::DELETE, &path, None).await;
    ended.ok(StatusCode::NO_CONTENT);
    assert_eq!(ended.header("tus-resumable"), Some("1.0.0"));
    assert!(f.objects(t.org).await.is_empty());
    assert_eq!(f.count("videos", t.org).await, 0);
    assert_eq!(f.count("course_videos", t.org).await, 0);
    f.send(Method::HEAD, &path, None)
        .await
        .ok(StatusCode::NOT_FOUND);
    let done = f.started(t.course, 30).await;
    f.patch(&done, 0, &mp4(30)).await.ok(StatusCode::NO_CONTENT);
    f.send(Method::DELETE, &done, None)
        .await
        .expect(StatusCode::CONFLICT, "video.status_invalid");
    assert_eq!(f.count("videos", t.org).await, 1);
}

/// API: a course's videos are listed page by page, read and renamed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn videos_are_listed_read_and_renamed(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let mut ids = Vec::new();
    for title in ["One", "Two"] {
        let created = f.create(t.course, 10, title).await;
        ids.push(created.ok(StatusCode::CREATED)["video"]["id"].clone());
    }
    let other = f.course(t.org, "S2").await;
    f.create(other, 10, "Elsewhere")
        .await
        .ok(StatusCode::CREATED);
    let list = format!("/api/v1/courses/{}/videos", t.course);
    let first = f.send(Method::GET, &format!("{list}?limit=1"), None).await;
    let first = first.ok(StatusCode::OK).clone();
    assert_schema("Page_Video", &first);
    assert_eq!(first["items"][0]["id"], ids[0]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = f
        .send(
            Method::GET,
            &format!("{list}?limit=1&cursor={cursor}"),
            None,
        )
        .await;
    let second = second.ok(StatusCode::OK).clone();
    assert_eq!(second["items"][0]["id"], ids[1]);
    assert_eq!(second["next_cursor"], Value::Null);

    let one = format!("/api/v1/videos/{}", ids[0].as_str().unwrap());
    let read = f.send(Method::GET, &one, None).await;
    assert_schema("Video", read.ok(StatusCode::OK));
    assert_eq!(read.body["chapters"], json!([]));
    let renamed = f
        .send(Method::PATCH, &one, Some(json!({ "title": " Week 1 " })))
        .await;
    assert_eq!(renamed.ok(StatusCode::OK)["title"], "Week 1");
    let unchanged = f.send(Method::PATCH, &one, Some(json!({}))).await;
    assert_eq!(unchanged.ok(StatusCode::OK)["title"], "Week 1");
    let blank = f
        .send(Method::PATCH, &one, Some(json!({ "title": "  " })))
        .await;
    blank.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    sqlx::query("UPDATE courses SET status = 'archived' WHERE id = $1")
        .bind(t.course)
        .execute(&f.pool)
        .await
        .unwrap();
    f.send(Method::PATCH, &one, Some(json!({ "title": "Late" })))
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    f.send(Method::GET, &format!("/api/v1/videos/{}", new_id()), None)
        .await
        .expect(StatusCode::NOT_FOUND, "video.not_found");
}

/// T16: org A's admin with org B's course, upload and video ids gets 404, and org B's rows and
/// objects are unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_another_organizations_ids_answer_404_and_change_nothing(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let b = tree(&f.pool, "org-b", f.now()).await;
    let admin_b = f.admin_of(b.org).await;
    // Org B's upload, with one chunk stored, made through org B's admin.
    let request = Request::post(format!("/api/v1/courses/{}/video-uploads", b.course))
        .header(TEST_ORG_HEADER, b.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, admin_b.clone())
        .header("tus-resumable", "1.0.0")
        .header("upload-length", 2 * MIB)
        .header("upload-metadata", "title Qg==")
        .body(Body::empty())
        .unwrap();
    let created = answer(&f.app, request).await;
    let path = created.header("location").unwrap().to_owned();
    let video = created.body["video"]["id"].as_str().unwrap().to_owned();
    let request = Request::patch(&path)
        .header(TEST_ORG_HEADER, b.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, admin_b)
        .header("tus-resumable", "1.0.0")
        .header(header::CONTENT_TYPE, "application/offset+octet-stream")
        .header("upload-offset", 0)
        .header(header::CONTENT_LENGTH, MIB)
        .body(Body::from(mp4(to_usize(MIB))))
        .unwrap();
    answer(&f.app, request).await.ok(StatusCode::NO_CONTENT);
    let objects_b = f.objects(b.org).await;
    assert_eq!(objects_b.len(), 1);

    f.create(b.course, 10, "Crossed")
        .await
        .expect(StatusCode::NOT_FOUND, "course.not_found");
    f.patch(&path, MIB, &mp4(to_usize(MIB)))
        .await
        .ok(StatusCode::NOT_FOUND);
    f.send(Method::HEAD, &path, None)
        .await
        .ok(StatusCode::NOT_FOUND);
    f.send(Method::DELETE, &path, None)
        .await
        .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
    let one = format!("/api/v1/videos/{video}");
    f.send(Method::GET, &one, None)
        .await
        .expect(StatusCode::NOT_FOUND, "video.not_found");
    f.send(Method::PATCH, &one, Some(json!({ "title": "Mine" })))
        .await
        .expect(StatusCode::NOT_FOUND, "video.not_found");
    f.send(
        Method::GET,
        &format!("/api/v1/courses/{}/videos", b.course),
        None,
    )
    .await
    .expect(StatusCode::NOT_FOUND, "course.not_found");
    assert_eq!(f.objects(b.org).await, objects_b);
    let row: (String, i64) = sqlx::query_as(
        "SELECT v.title, u.received FROM videos v JOIN video_uploads u ON u.video_id = v.id
         WHERE v.org_id = $1",
    )
    .bind(b.org)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(row, ("B".to_owned(), i64::try_from(MIB).unwrap()));
    assert_eq!(f.count("videos", f.a.org).await, 0);
}

/// M1 spec 02 Q13 (#312): on a draft course, a holder of `course.content_edit` without
/// `course.view_all` gets 404 from every upload and video route, before the route's permission,
/// and nothing changes; once the course is published the same caller uploads and renames.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_draft_courses_uploads_and_videos_are_not_found_without_view_all(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    let path = f.started(t.course, 30).await;
    let video: Uuid = sqlx::query_scalar("SELECT video_id FROM video_uploads WHERE id = $1")
        .bind(upload_id(&path))
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let editor = f.holder(&["course.content_edit"]).await;
    let viewer = f.holder(&["course.view"]).await;
    let as_caller = |cookie: &str, method: Method, uri: &str| {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, t.org.to_string())
            .header("sec-fetch-site", "same-origin")
            .header(header::COOKIE, cookie)
            .header("tus-resumable", "1.0.0")
    };
    let create = format!("/api/v1/courses/{}/video-uploads", t.course);
    let one = format!("/api/v1/videos/{video}");
    for cookie in [&editor, &viewer] {
        let request = as_caller(cookie, Method::POST, &create)
            .header("upload-length", 10)
            .header("upload-metadata", "title eA==")
            .body(Body::empty())
            .unwrap();
        answer(&f.app, request)
            .await
            .expect(StatusCode::NOT_FOUND, "course.not_found");
        let request = as_caller(cookie, Method::PATCH, &path)
            .header(header::CONTENT_TYPE, "application/offset+octet-stream")
            .header("upload-offset", 0)
            .header(header::CONTENT_LENGTH, 30)
            .body(Body::from(mp4(30)))
            .unwrap();
        answer(&f.app, request)
            .await
            .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
        let request = as_caller(cookie, Method::HEAD, &path)
            .body(Body::empty())
            .unwrap();
        answer(&f.app, request).await.ok(StatusCode::NOT_FOUND);
        let request = as_caller(cookie, Method::DELETE, &path)
            .body(Body::empty())
            .unwrap();
        answer(&f.app, request)
            .await
            .expect(StatusCode::NOT_FOUND, "video_upload.not_found");
        let request = as_caller(cookie, Method::PATCH, &one)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"title":"Mine"}"#))
            .unwrap();
        answer(&f.app, request)
            .await
            .expect(StatusCode::NOT_FOUND, "video.not_found");
    }
    assert_eq!(f.count("videos", t.org).await, 1);
    assert!(f.objects(t.org).await.is_empty());
    sqlx::query("UPDATE courses SET status = 'published' WHERE id = $1")
        .bind(t.course)
        .execute(&f.pool)
        .await
        .unwrap();
    let request = as_caller(&editor, Method::PATCH, &path)
        .header(header::CONTENT_TYPE, "application/offset+octet-stream")
        .header("upload-offset", 0)
        .header(header::CONTENT_LENGTH, 30)
        .body(Body::from(mp4(30)))
        .unwrap();
    assert_eq!(answer(&f.app, request).await.offset(), 30);
    let request = as_caller(&editor, Method::PATCH, &one)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"Mine"}"#))
        .unwrap();
    let renamed = answer(&f.app, request).await;
    assert_eq!(renamed.ok(StatusCode::OK)["title"], "Mine");
}

/// A request to `uri` as the holder of `cookie` in org A, tus header included.
fn as_caller(f: &Fixture, cookie: &str, method: Method, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(TEST_ORG_HEADER, f.a.org.to_string())
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookie)
        .header("tus-resumable", "1.0.0")
}

/// Set the course's status by SQL.
async fn set_status(f: &Fixture, course: Uuid, status: &str) {
    sqlx::query("UPDATE courses SET status = $2 WHERE id = $1")
        .bind(course)
        .bind(status)
        .execute(&f.pool)
        .await
        .expect("set the status");
}

/// A new account under org A's root.
async fn account(f: &Fixture, name: &str) -> Uuid {
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
            .bind(f.a.org)
            .fetch_one(&f.pool)
            .await
            .expect("the root");
    accounts::create(
        &f.pool,
        scope(f.a.org),
        root,
        name,
        f.now(),
        Actor::Instance,
    )
    .await
    .expect("create account")
    .id
}

/// API (`VideoTarget`, D9, D11): a video attached to a course in Tower and to one in Apron, as
/// a course copy will attach it. A caller who can see only the Tower course and lacks the
/// permission there gets 403; one who holds it there gets 200, the Apron course hidden; one who
/// can see neither gets 404. A rename passes while one attached course is open and answers 409
/// once both are archived.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_video_of_two_courses_is_checked_at_each(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    set_status(&f, t.course, "published").await;
    let apron = account(&f, "Apron").await;
    let ground = account(&f, "Ground").await;
    let new = courses::NewCourse {
        account_id: apron,
        title: "Course S2".into(),
        code: "S2".into(),
    };
    let other = courses::create(&f.pool, scope(t.org), &new, f.now(), Actor::Instance)
        .await
        .expect("create course")
        .id;
    set_status(&f, other, "published").await;
    let path = f.started(t.course, 30).await;
    let video = f.video_of(&path).await;
    sqlx::query("INSERT INTO course_videos (org_id, course_id, video_id) VALUES ($1, $2, $3)")
        .bind(t.org)
        .bind(other)
        .bind(video)
        .execute(&f.pool)
        .await
        .expect("attach to the second course");
    let tower: Uuid = sqlx::query_scalar("SELECT account_id FROM courses WHERE id = $1")
        .bind(t.course)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let one = format!("/api/v1/videos/{video}");
    let read = async |cookie: &str| {
        let request = as_caller(&f, cookie, Method::GET, &one)
            .body(Body::empty())
            .unwrap();
        answer(&f.app, request).await
    };
    let viewer = f.holder_at(&["course.view"], Scope::Account(tower)).await;
    read(&viewer)
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    let staff = f
        .holder_at(&["course.view_all"], Scope::Account(tower))
        .await;
    assert_eq!(
        read(&staff).await.ok(StatusCode::OK)["id"],
        video.to_string()
    );
    let elsewhere = f
        .holder_at(&["course.view_all"], Scope::Account(ground))
        .await;
    read(&elsewhere)
        .await
        .expect(StatusCode::NOT_FOUND, "video.not_found");

    set_status(&f, other, "archived").await;
    let renamed = f
        .send(
            Method::PATCH,
            &one,
            Some(json!({ "title": "Still open in Tower" })),
        )
        .await;
    assert_eq!(renamed.ok(StatusCode::OK)["title"], "Still open in Tower");
    set_status(&f, t.course, "archived").await;
    f.send(
        Method::PATCH,
        &one,
        Some(json!({ "title": "Both archived" })),
    )
    .await
    .expect(StatusCode::CONFLICT, "course.archived");
    let title: String = sqlx::query_scalar("SELECT title FROM videos WHERE id = $1")
        .bind(video)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(title, "Still open in Tower");
}

/// Spec m1/02 (#318): a Student enrolled in a published course sees it but holds neither
/// `course.content_edit` nor `course.view_all`, so every upload and video route answers 403 and
/// nothing changes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrolled_student_is_refused_every_upload_and_video_route(pool: PgPool) {
    let f = Fixture::new(pool, Backend::Local).await;
    let t = f.a;
    set_status(&f, t.course, "published").await;
    let path = f.started(t.course, 2 * MIB).await;
    let video = f.video_of(&path).await;
    let student_role: Uuid =
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = 'student'")
            .fetch_one(&f.pool)
            .await
            .expect("the Student role");
    let student = f.user(t.org).await;
    let enrollment = enrollments::NewEnrollment {
        user_id: student,
        role_id: student_role,
        source: Source::Local,
        external_id: None,
    };
    enrollments::create(
        &f.pool,
        scope(t.org),
        t.course,
        &enrollment,
        f.now(),
        Actor::Instance,
    )
    .await
    .expect("enroll the student");
    let cookie = f.session(t.org, student).await;
    let denied = |answer: Answer| answer.expect(StatusCode::FORBIDDEN, "permission.denied");
    let create = format!("/api/v1/courses/{}/video-uploads", t.course);
    let request = as_caller(&f, &cookie, Method::POST, &create)
        .header("upload-length", 10)
        .header("upload-metadata", "title eA==")
        .body(Body::empty())
        .unwrap();
    denied(answer(&f.app, request).await);
    let request = as_caller(&f, &cookie, Method::PATCH, &path)
        .header(header::CONTENT_TYPE, "application/offset+octet-stream")
        .header("upload-offset", 0)
        .header(header::CONTENT_LENGTH, MIB)
        .body(Body::from(mp4(to_usize(MIB))))
        .unwrap();
    denied(answer(&f.app, request).await);
    let request = as_caller(&f, &cookie, Method::HEAD, &path)
        .body(Body::empty())
        .unwrap();
    answer(&f.app, request).await.ok(StatusCode::FORBIDDEN);
    let request = as_caller(&f, &cookie, Method::DELETE, &path)
        .body(Body::empty())
        .unwrap();
    denied(answer(&f.app, request).await);
    let one = format!("/api/v1/videos/{video}");
    let request = as_caller(&f, &cookie, Method::GET, &one)
        .body(Body::empty())
        .unwrap();
    denied(answer(&f.app, request).await);
    let request = as_caller(&f, &cookie, Method::PATCH, &one)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"title":"Mine"}"#))
        .unwrap();
    denied(answer(&f.app, request).await);
    let list = format!("/api/v1/courses/{}/videos", t.course);
    let request = as_caller(&f, &cookie, Method::GET, &list)
        .body(Body::empty())
        .unwrap();
    denied(answer(&f.app, request).await);
    assert_eq!(f.count("videos", t.org).await, 1);
    assert!(f.objects(t.org).await.is_empty());
}
