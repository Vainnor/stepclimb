// SPDX-License-Identifier: AGPL-3.0-or-later
//! Playback sessions, media delivery, `video` items, deleting and detaching videos over the router
//! (docs/specs/m1/08-uploaded-video.md B3, B17 to B20, B18a; T11, T12, T16 and T18 for these
//! routes). The permission suite's generated cases (T15, T16) are in `tests/permissions.rs`, and
//! the session routes' lock suite entries in `tests/lock_suite/mod.rs`.
//!
//! No ffmpeg runs here: a ready video is a `videos` row set `ready` by SQL, as a successful
//! transcode leaves it, with stand-in outputs (a few bytes each) written to a temporary local
//! store under its `v<n>/` prefix. Needs `DATABASE_URL` (`just up`).

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
use object_store::{ObjectStoreExt as _, PutPayload};
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
    videos::{self, NewUpload},
};
use stepclimb_jobs::{Worker, backoff::Random};
use stepclimb_server::{
    AppState,
    config::StorageConfig,
    file_delivery::SANDBOX_POLICY,
    jobs,
    media_urls::{self, MediaGrant},
    request_org::TEST_ORG_HEADER,
    router, session,
    storage::Storage,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// The origin the server allows besides its own (`allowed_origins`), as a client on another
/// origin is.
const APP_ORIGIN: &str = "https://app.example.org";

/// 2025-11-13T02:13:20Z, a whole second.
fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs, the admin routes and the transcode that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// An organization with a published course in its Tower account and a published module.
#[derive(Clone, Copy, Debug)]
struct Org {
    id: Uuid,
    tower: Uuid,
    course: Uuid,
    module: Uuid,
}

/// The stand-in outputs of a ready video with one 360p rendition, relative to `v<n>/`.
const OUTPUTS: [(&str, &[u8]); 8] = [
    (
        "master.m3u8",
        b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=928000\n360p/index.m3u8\n",
    ),
    (
        "360p/index.m3u8",
        b"#EXTM3U\n#EXTINF:6.0,\nsegment_00000.ts\n#EXTINF:6.0,\nsegment_00001.ts\n",
    ),
    ("360p/segment_00000.ts", b"\x47first segment bytes"),
    ("360p/segment_00001.ts", b"\x47second segment bytes"),
    ("video.mp4", b"\0\0\0\x18ftypisom mp4 bytes"),
    ("poster.jpg", b"\xff\xd8\xff poster"),
    (
        "thumbnails.vtt",
        b"WEBVTT\n\n00:00.000 --> 00:10.000\nsprite_000.jpg#xywh=0,0,160,90\n",
    ),
    ("sprite_000.jpg", b"\xff\xd8\xff sprite"),
];

struct Fixture {
    pool: PgPool,
    app: Router,
    clock: ManualClock,
    storage: Storage,
    _dir: TempDir,
    a: Org,
    admin: (Uuid, String),
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, start())
            .await
            .expect("sync roles");
        let dir = std::env::temp_dir().join(format!("stepclimb-playback-{}", new_id()));
        std::fs::create_dir_all(&dir).expect("create the directory");
        let storage = Storage::open(&StorageConfig::Local { root: dir.clone() }, 64 * 1024)
            .expect("open the store")
            .expect("storage is configured");
        let clock = ManualClock::new(start());
        let app = app(&pool, &clock, storage.clone());
        let a = org(&pool, "org-a").await;
        let mut fixture = Self {
            pool,
            app,
            clock,
            storage,
            _dir: TempDir(dir),
            a,
            admin: (Uuid::nil(), String::new()),
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

    async fn holder(&self, org: Uuid, key: &str, at: Scope) -> (Uuid, String) {
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
        (user, cookie)
    }

    /// A user enrolled in the course `course` of `org` with the built-in role `key`: its cookie.
    async fn enrolled(&self, org: Uuid, course: Uuid, key: &str) -> String {
        let (user, cookie) = self.user(org).await;
        let new = enrollments::NewEnrollment {
            user_id: user,
            role_id: self.role(key).await,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(
            &self.pool,
            scope(org),
            course,
            &new,
            start(),
            Actor::Instance,
        )
        .await
        .expect("enroll");
        cookie
    }

    async fn send(
        &self,
        (method, uri): (Method, &str),
        org: Uuid,
        cookie: Option<&str>,
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
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        answer(&self.app, request.body(Body::empty()).expect("a request")).await
    }

    /// An uploaded video attached to `course` of `org`, `uploading` with its upload row.
    async fn uploading(&self, org: Uuid, course: Uuid, by: Uuid) -> Uuid {
        let mut tx = self.pool.begin().await.expect("a transaction");
        let new = NewUpload {
            course_id: course,
            title: "Week 1 lecture",
            length: 1024,
            created_by: by,
        };
        let (video, _) = videos::create_upload(&mut tx, scope(org), &new, start())
            .await
            .expect("create the upload");
        tx.commit().await.expect("commit");
        video.id
    }

    /// A video of `course` in `org` as a successful transcode run 1 leaves it: `ready`, its
    /// upload row gone, the stand-in [`OUTPUTS`] stored under `v1/`.
    async fn ready(&self, org: Uuid, course: Uuid) -> Uuid {
        let video = self.uploading(org, course, self.admin_of(org).await).await;
        self.make_ready(org, video, 1).await;
        video
    }

    /// The video `video` of `org` set `ready` at run `run` with the stand-in outputs stored.
    async fn make_ready(&self, org: Uuid, video: Uuid, run: i32) {
        sqlx::query("DELETE FROM video_uploads WHERE video_id = $1")
            .bind(video)
            .execute(&self.pool)
            .await
            .expect("drop the upload");
        sqlx::query(
            "UPDATE videos SET status = 'ready', output_run = $2, duration_s = 12,
                    renditions = '{\"heights\": [360]}'::jsonb, poster_key = 'poster.jpg'
             WHERE id = $1",
        )
        .bind(video)
        .bind(run)
        .execute(&self.pool)
        .await
        .expect("set the video ready");
        for (name, bytes) in OUTPUTS {
            self.put(org, video, &format!("v{run}/{name}"), bytes).await;
        }
    }

    async fn put(&self, org: Uuid, video: Uuid, key: &str, bytes: &[u8]) {
        let path = self
            .storage
            .path(&format!("orgs/{org}/videos/{video}/{key}"));
        self.storage
            .store()
            .put(&path, PutPayload::from(bytes.to_vec()))
            .await
            .expect("store an output");
    }

    /// Every object key under the video's prefix, sorted.
    async fn objects(&self, org: Uuid, video: Uuid) -> Vec<String> {
        let mut keys = Vec::new();
        let mut pending = vec![self.storage.path(&format!("orgs/{org}/videos/{video}"))];
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

    /// The organization admin of org A, or any user of `org` to start an upload with.
    async fn admin_of(&self, org: Uuid) -> Uuid {
        if org == self.a.id {
            return self.admin.0;
        }
        sqlx::query_scalar("SELECT id FROM users WHERE org_id = $1 LIMIT 1")
            .bind(org)
            .fetch_one(&self.pool)
            .await
            .expect("a user of the organization")
    }

    /// A published `video` item of `module` in `org` naming `video`.
    async fn video_item(&self, org: Uuid, module: Uuid, video: Uuid) -> Uuid {
        let new = items::NewItem {
            kind: items::Kind::Video,
            title: "Lecture".into(),
            ref_id: Some(video),
            url: None,
            position: None,
        };
        let item = items::create(&self.pool, scope(org), module, &new, start())
            .await
            .expect("create item")
            .id;
        let publish = items::ItemPatch {
            published: Some(true),
            ..Default::default()
        };
        items::update(&self.pool, scope(org), item, &publish, start())
            .await
            .expect("publish item");
        item
    }

    /// One pass of a `default` queue worker: how many jobs it ran.
    async fn work(&self) -> usize {
        let registry = jobs::registry(Some(self.storage.clone()), None).expect("the registry");
        let worker = Worker::new(
            self.pool.clone(),
            registry,
            self.clock.clock(),
            Random::system(),
            vec!["default"],
        );
        worker.run_once(4).await.expect("a pass")
    }

    async fn video_exists(&self, video: Uuid) -> bool {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM videos WHERE id = $1)")
            .bind(video)
            .fetch_one(&self.pool)
            .await
            .expect("read")
    }
}

/// The router of a server with `storage`, allowing [`APP_ORIGIN`], on `clock`.
fn app(pool: &PgPool, clock: &ManualClock, storage: Storage) -> Router {
    let state = AppState {
        db: pool.clone(),
        clock: clock.clock(),
        keys: KeyRing::parse(KEYS).expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: vec![WebOrigin::parse(APP_ORIGIN).expect("a valid origin")],
        rate_limits: Default::default(),
        passwords: Default::default(),
        storage: Some(storage),
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    };
    router(state).layer(Extension(ConnectInfo(CLIENT)))
}

/// What follows the public URL in `url`.
fn path_of(url: &Value) -> String {
    url.as_str()
        .expect("a url")
        .strip_prefix("https://lms.example.org")
        .expect("the public URL")
        .to_owned()
}

/// The token of a session's `hls_url`.
fn token_of(session: &Value) -> String {
    path_of(&session["hls_url"])
        .strip_prefix("/media/")
        .and_then(|rest| rest.strip_suffix("/master.m3u8"))
        .expect("a playback URL")
        .to_owned()
}

async fn org(pool: &PgPool, name: &str) -> Org {
    let slug = format!("{name}-{}", &new_id().simple().to_string()[24..]);
    let id = organizations::create(pool, &slug, &slug, start())
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
    let (course, module) = course(pool, id, tower, "S1").await;
    Org {
        id,
        tower,
        course,
        module,
    }
}

/// A published course coded `code` in `account` of `org` with one published module.
async fn course(pool: &PgPool, org: Uuid, account: Uuid, code: &str) -> (Uuid, Uuid) {
    let new = courses::NewCourse {
        account_id: account,
        title: format!("Course {code}"),
        code: code.into(),
    };
    let course = courses::create(pool, scope(org), &new, start(), Actor::Instance)
        .await
        .expect("create course")
        .id;
    courses::set_status(
        pool,
        scope(org),
        course,
        courses::Status::Published,
        start(),
        Actor::Instance,
    )
    .await
    .expect("publish the course");
    let module = modules::create(pool, scope(org), course, "Module", start())
        .await
        .expect("create module")
        .id;
    let publish = modules::ModulePatch {
        published: Some(true),
        ..Default::default()
    };
    modules::update(pool, scope(org), module, &publish, start())
        .await
        .expect("publish module");
    (course, module)
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

/// T11 (B17, B18): a Student's session for an unlocked item plays the master playlist, the
/// rendition's playlist and every segment, the MP4, the poster and the thumbnails through the
/// one token, each as its type with B18's headers.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_a_students_session_plays_every_output_through_one_token(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let item = f.video_item(a.id, a.module, video).await;
    let student = f.enrolled(a.id, a.course, "student").await;
    let uri = format!("/api/v1/items/{item}/video-session");
    let session = f
        .send((Method::POST, &uri), a.id, Some(&student), &[])
        .await
        .ok();
    assert_schema("VideoSession", &session);
    assert_eq!(session["duration_s"], 12.0);
    assert_eq!(session["captions"], json!([]));
    assert_eq!(
        session["expires_at"], "2025-11-13T04:13:20.000000Z",
        "2 hours"
    );
    let token = token_of(&session);
    for (field, name) in [
        ("mp4_url", "video.mp4"),
        ("poster_url", "poster.jpg"),
        ("thumbnails_url", "thumbnails.vtt"),
    ] {
        assert_eq!(path_of(&session[field]), format!("/media/{token}/{name}"));
    }
    let types = [
        ("master.m3u8", "application/vnd.apple.mpegurl"),
        ("360p/index.m3u8", "application/vnd.apple.mpegurl"),
        ("360p/segment_00000.ts", "video/mp2t"),
        ("360p/segment_00001.ts", "video/mp2t"),
        ("video.mp4", "video/mp4"),
        ("poster.jpg", "image/jpeg"),
        ("thumbnails.vtt", "text/vtt; charset=utf-8"),
        ("sprite_000.jpg", "image/jpeg"),
    ];
    for ((name, mime), (_, bytes)) in types.iter().zip(OUTPUTS) {
        let got = f
            .send(
                (Method::GET, &format!("/media/{token}/{name}")),
                a.id,
                None,
                &[],
            )
            .await;
        assert_eq!(got.status, StatusCode::OK, "{name}: {got:?}");
        assert_eq!(got.bytes, bytes, "{name}");
        assert_eq!(got.header(header::CONTENT_TYPE), *mime, "{name}");
        assert_eq!(got.header(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
        assert_eq!(got.header(header::CONTENT_SECURITY_POLICY), SANDBOX_POLICY);
        assert_eq!(got.header(header::CACHE_CONTROL), "private, max-age=7200");
        assert_eq!(got.header(header::ACCEPT_RANGES), "bytes");
        assert_eq!(
            got.header(header::HeaderName::from_static(
                "cross-origin-resource-policy"
            )),
            "same-origin"
        );
        assert_eq!(got.header(header::REFERRER_POLICY), "no-referrer");
        let attachment = got.header(header::CONTENT_DISPOSITION);
        assert_eq!(attachment.starts_with("attachment"), *name == "video.mp4");
    }
}

/// T11 (B18): under a good token, a path that is not an output of the video's run answers 404:
/// `..`, another video's id, a rendition the video lacks, the upload's parts, a file outside the
/// layout. So does a token 1 s past its expiry, and an output whose object is gone.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_only_the_runs_outputs_are_served_and_only_until_expiry(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let other = f.ready(a.id, a.course).await;
    f.put(a.id, video, "upload/0", b"a stray part").await;
    f.put(a.id, video, "v1/secret.txt", b"not an output").await;
    let uri = format!("/api/v1/videos/{video}/preview-session");
    let session = f
        .send((Method::POST, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .ok();
    let token = token_of(&session);
    for path in [
        "../master.m3u8".to_owned(),
        "..%2Fv1%2Fmaster.m3u8".to_owned(),
        "360p/..%2F..%2Fupload%2F0".to_owned(),
        format!("{other}/master.m3u8"),
        "720p/index.m3u8".to_owned(),
        "upload/0".to_owned(),
        "secret.txt".to_owned(),
        "360p/segment_00002.ts".to_owned(),
    ] {
        f.send(
            (Method::GET, &format!("/media/{token}/{path}")),
            a.id,
            None,
            &[],
        )
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    }
    let master = format!("/media/{token}/master.m3u8");
    f.clock.advance(media_urls::TTL - Duration::from_secs(1));
    let last = f.send((Method::GET, &master), a.id, None, &[]).await;
    assert_eq!(last.status, StatusCode::OK, "{last:?}");
    assert_eq!(last.header(header::CACHE_CONTROL), "private, max-age=1");
    f.clock.advance(Duration::from_secs(2));
    f.send((Method::GET, &master), a.id, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
}

/// B18: a token names its run. Once the video is transcoded again (run 2) or deleted, the old
/// token serves nothing; a token for a video that is not `ready` serves nothing either.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_token_serves_only_its_ready_run(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let uri = format!("/api/v1/videos/{video}/preview-session");
    let session = f
        .send((Method::POST, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .ok();
    let master = path_of(&session["hls_url"]);
    f.make_ready(a.id, video, 2).await;
    f.send((Method::GET, &master), a.id, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    let fresh = f
        .send((Method::POST, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .ok();
    let fresh = path_of(&fresh["hls_url"]);
    assert_eq!(
        f.send((Method::GET, &fresh), a.id, None, &[]).await.status,
        StatusCode::OK
    );
    sqlx::query("UPDATE videos SET status = 'processing' WHERE id = $1")
        .bind(video)
        .execute(&f.pool)
        .await
        .unwrap();
    f.send((Method::GET, &fresh), a.id, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
}

/// T16 (B18): a token issued in org B fetched through org A, and org A's token through org B,
/// answer 404, with both organizations' objects in place; a token sealed for a signed file URL
/// does not open as a playback token.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_token_serves_only_its_own_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let b = org(&f.pool, "org-b").await;
    let (_, b_admin) = f.holder(b.id, "org_admin", Scope::Organization).await;
    let mine = f.ready(a.id, a.course).await;
    let theirs = f.ready(b.id, b.course).await;
    let session_of = |org: Uuid, video: Uuid, cookie: String| {
        let f = &f;
        async move {
            let uri = format!("/api/v1/videos/{video}/preview-session");
            let session = f
                .send((Method::POST, &uri), org, Some(&cookie), &[])
                .await
                .ok();
            path_of(&session["hls_url"])
        }
    };
    let a_master = session_of(a.id, mine, f.admin.1.clone()).await;
    let b_master = session_of(b.id, theirs, b_admin).await;
    f.send((Method::GET, &b_master), a.id, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    f.send((Method::GET, &a_master), b.id, None, &[])
        .await
        .expect(StatusCode::NOT_FOUND, "file.not_found");
    // A grant naming org B's video, sealed by this server for org A, reaches nothing.
    let forged = MediaGrant {
        org_id: a.id,
        video_id: theirs,
        run: 1,
        expires_at: 1_763_003_600,
    };
    let keys = KeyRing::parse(KEYS).expect("the ring");
    let token = media_urls::seal(&keys, &forged).expect("seal");
    f.send(
        (Method::GET, &format!("/media/{token}/master.m3u8")),
        a.id,
        None,
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND, "file.not_found");
    assert_eq!(f.objects(b.id, theirs).await.len(), OUTPUTS.len());
}

/// B17: a session needs a `ready` video (409 `video.not_ready`) and a `video` item (422
/// `item.not_a_video`); B18 serves one range and answers `HEAD` without the body.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_session_needs_a_ready_video_and_ranges_are_served(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let pending = f.uploading(a.id, a.course, f.admin.0).await;
    let item = f.video_item(a.id, a.module, pending).await;
    let student = f.enrolled(a.id, a.course, "student").await;
    f.send(
        (Method::POST, &format!("/api/v1/items/{item}/video-session")),
        a.id,
        Some(&student),
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT, "video.not_ready");
    f.send(
        (
            Method::POST,
            &format!("/api/v1/videos/{pending}/preview-session"),
        ),
        a.id,
        Some(&f.admin.1),
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT, "video.not_ready");
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
    let answer = f
        .send(
            (
                Method::POST,
                &format!("/api/v1/items/{header}/video-session"),
            ),
            a.id,
            Some(&f.admin.1),
            &[],
        )
        .await;
    answer.expect(StatusCode::UNPROCESSABLE_ENTITY, "request.invalid");
    assert_eq!(answer.json()["errors"][0]["code"], "item.not_a_video");
    let video = f.ready(a.id, a.course).await;
    let uri = format!("/api/v1/videos/{video}/preview-session");
    let session = f
        .send((Method::POST, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .ok();
    let segment = path_of(&session["hls_url"]).replace("master.m3u8", "360p/segment_00000.ts");
    let part = f
        .send(
            (Method::GET, &segment),
            a.id,
            None,
            &[(header::RANGE, "bytes=1-5")],
        )
        .await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT, "{part:?}");
    assert_eq!(part.bytes, b"first");
    assert_eq!(part.header(header::CONTENT_RANGE), "bytes 1-5/20");
    let head = f.send((Method::HEAD, &segment), a.id, None, &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.bytes.is_empty());
    assert_eq!(head.header(header::CONTENT_LENGTH), "20");
    f.send(
        (Method::GET, &segment),
        a.id,
        None,
        &[(header::RANGE, "bytes=20-")],
    )
    .await
    .expect(StatusCode::RANGE_NOT_SATISFIABLE, "file.range_invalid");
}

/// T18 (B18a): a media or file URL fetched with an allowed `Origin` gets it echoed with
/// `Vary: Origin` and the range headers exposed; another `Origin` gets no CORS header; a preflight
/// for `GET` with `Range` is answered, one for `POST` or another header is not; no answer allows
/// credentials.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_signed_urls_answer_allowed_origins_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let uri = format!("/api/v1/videos/{video}/preview-session");
    let session = f
        .send((Method::POST, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .ok();
    let master = path_of(&session["hls_url"]);
    let allow = header::ACCESS_CONTROL_ALLOW_ORIGIN;
    for path in [master.as_str(), "/files/not-a-token"] {
        let allowed = f
            .send(
                (Method::GET, path),
                a.id,
                None,
                &[(header::ORIGIN, APP_ORIGIN)],
            )
            .await;
        assert_eq!(
            allowed.header(allow.clone()),
            APP_ORIGIN,
            "{path}: {allowed:?}"
        );
        assert_eq!(allowed.header(header::VARY), "origin", "{path}");
        assert!(
            allowed
                .header(header::ACCESS_CONTROL_EXPOSE_HEADERS)
                .contains("content-range"),
            "{path}"
        );
        let other = f
            .send(
                (Method::GET, path),
                a.id,
                None,
                &[(header::ORIGIN, "https://evil.example")],
            )
            .await;
        assert!(other.headers.get(allow.clone()).is_none(), "{path}");
        assert_eq!(other.header(header::VARY), "origin", "{path}");
        for answer in [&allowed, &other] {
            assert!(
                answer
                    .headers
                    .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                    .is_none()
            );
        }
    }
    let preflight = |method: &'static str, headers: &'static str, origin: &'static str| {
        let f = &f;
        let master = master.clone();
        async move {
            f.send(
                (Method::OPTIONS, &master),
                a.id,
                None,
                &[
                    (header::ORIGIN, origin),
                    (header::ACCESS_CONTROL_REQUEST_METHOD, method),
                    (header::ACCESS_CONTROL_REQUEST_HEADERS, headers),
                ],
            )
            .await
        }
    };
    let good = preflight("GET", "range", APP_ORIGIN).await;
    assert_eq!(good.status, StatusCode::NO_CONTENT, "{good:?}");
    assert_eq!(good.header(allow.clone()), APP_ORIGIN);
    assert_eq!(good.header(header::ACCESS_CONTROL_ALLOW_HEADERS), "range");
    assert_eq!(
        good.header(header::ACCESS_CONTROL_ALLOW_METHODS),
        "GET, HEAD"
    );
    assert!(
        good.headers
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none()
    );
    for (method, headers, origin) in [
        ("POST", "range", APP_ORIGIN),
        ("GET", "range, authorization", APP_ORIGIN),
        ("GET", "range", "https://evil.example"),
    ] {
        let refused = preflight(method, headers, origin).await;
        assert!(
            refused.headers.get(allow.clone()).is_none(),
            "{method} {headers} {origin}: {refused:?}"
        );
    }
}

/// B3: a `video` item names a video attached to its course in any status; another course's video,
/// or another organization's, answers 404 `video.not_found`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b3_a_video_item_names_a_video_of_its_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let (other_course, _) = course(&f.pool, a.id, a.tower, "S2").await;
    let b = org(&f.pool, "org-b").await;
    f.holder(b.id, "org_admin", Scope::Organization).await;
    let uploading = f.uploading(a.id, a.course, f.admin.0).await;
    let elsewhere = f.ready(a.id, other_course).await;
    let theirs = f.ready(b.id, b.course).await;
    let uri = format!("/api/v1/modules/{}/items", a.module);
    let create = |video: Uuid| {
        let f = &f;
        let uri = uri.clone();
        async move {
            let request = Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(TEST_ORG_HEADER, a.id.to_string())
                .header("sec-fetch-site", "same-origin")
                .header(header::COOKIE, &f.admin.1)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "kind": "video", "title": "Lecture", "ref_id": video }).to_string(),
                ))
                .expect("a request");
            answer(&f.app, request).await
        }
    };
    let made = create(uploading).await;
    assert_eq!(made.status, StatusCode::CREATED, "{made:?}");
    assert_eq!(made.json()["ref_id"], uploading.to_string());
    for video in [elsewhere, theirs] {
        create(video)
            .await
            .expect(StatusCode::NOT_FOUND, "video.not_found");
    }
}

/// T12 (B20): deleting a video an item names answers 409 `video.in_use`; once the item is gone
/// the delete answers 204, the row goes with its attachments, and `videos.purge` deletes every
/// object under its prefix after the commit. A processing video answers 409
/// `video.status_invalid`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_a_video_in_use_or_processing_is_not_deleted(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    f.put(a.id, video, "upload/0", b"a late part").await;
    let item = f.video_item(a.id, a.module, video).await;
    let uri = format!("/api/v1/videos/{video}");
    f.send((Method::DELETE, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .expect(StatusCode::CONFLICT, "video.in_use");
    items::delete(&f.pool, scope(a.id), item)
        .await
        .expect("delete the item");
    let answer = f
        .send((Method::DELETE, &uri), a.id, Some(&f.admin.1), &[])
        .await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{answer:?}");
    assert!(!f.video_exists(video).await);
    assert_eq!(
        f.objects(a.id, video).await.len(),
        OUTPUTS.len() + 1,
        "kept until the job runs"
    );
    assert_eq!(f.work().await, 1);
    assert!(f.objects(a.id, video).await.is_empty());
    let processing = f.uploading(a.id, a.course, f.admin.0).await;
    sqlx::query("UPDATE videos SET status = 'processing' WHERE id = $1")
        .bind(processing)
        .execute(&f.pool)
        .await
        .unwrap();
    f.send(
        (Method::DELETE, &format!("/api/v1/videos/{processing}")),
        a.id,
        Some(&f.admin.1),
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT, "video.status_invalid");
    assert!(f.video_exists(processing).await);
}

/// T12 (B20, B1): a course copy is attached to the same video, and its `video` item names it.
/// Detaching from a course whose item names it answers 409; detaching from a course while
/// another remains keeps the video; detaching from the last deletes it, and the job deletes its
/// objects.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t12_detaching_from_the_last_course_deletes_the_video(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let item = f.video_item(a.id, a.module, video).await;
    let copy = courses::CourseCopy {
        account_id: a.tower,
        title: None,
        code: "S1-COPY".into(),
        template: false,
    };
    let copied = courses::copy(
        &f.pool,
        scope(a.id),
        a.course,
        &copy,
        start(),
        Actor::Instance,
    )
    .await
    .expect("copy the course")
    .id;
    let copied_ref: Option<Uuid> = sqlx::query_scalar(
        "SELECT ref_id FROM module_items WHERE course_id = $1 AND kind = 'video'",
    )
    .bind(copied)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        copied_ref,
        Some(video),
        "the copy's item names the same video"
    );
    let detach = |course: Uuid| format!("/api/v1/courses/{course}/videos/{video}");
    f.send(
        (Method::DELETE, &detach(a.course)),
        a.id,
        Some(&f.admin.1),
        &[],
    )
    .await
    .expect(StatusCode::CONFLICT, "video.in_use");
    items::delete(&f.pool, scope(a.id), item)
        .await
        .expect("delete the item");
    let first = f
        .send(
            (Method::DELETE, &detach(a.course)),
            a.id,
            Some(&f.admin.1),
            &[],
        )
        .await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{first:?}");
    assert!(f.video_exists(video).await, "still attached to the copy");
    assert_eq!(f.work().await, 0, "nothing to delete yet");
    f.send(
        (Method::DELETE, &detach(a.course)),
        a.id,
        Some(&f.admin.1),
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND, "video.not_found");
    let copy_item: Uuid =
        sqlx::query_scalar("SELECT id FROM module_items WHERE course_id = $1 AND kind = 'video'")
            .bind(copied)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    items::delete(&f.pool, scope(a.id), copy_item)
        .await
        .expect("delete the copy's item");
    let last = f
        .send(
            (Method::DELETE, &detach(copied)),
            a.id,
            Some(&f.admin.1),
            &[],
        )
        .await;
    assert_eq!(last.status, StatusCode::NO_CONTENT, "{last:?}");
    assert!(!f.video_exists(video).await);
    assert_eq!(f.work().await, 1);
    assert!(f.objects(a.id, video).await.is_empty());
}

/// B20: a delete needs `course.content_edit` at every course the video is attached to (403
/// otherwise) and none archived (409 `course.archived`).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_delete_needs_every_course_editable(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let (second, _) = course(&f.pool, a.id, a.tower, "S2").await;
    sqlx::query("INSERT INTO course_videos (org_id, course_id, video_id) VALUES ($1, $2, $3)")
        .bind(a.id)
        .bind(second)
        .bind(video)
        .execute(&f.pool)
        .await
        .unwrap();
    let designer = f.enrolled(a.id, a.course, "course_designer").await;
    let uri = format!("/api/v1/videos/{video}");
    f.send((Method::DELETE, &uri), a.id, Some(&designer), &[])
        .await
        .expect(StatusCode::FORBIDDEN, "permission.denied");
    courses::set_status(
        &f.pool,
        scope(a.id),
        second,
        courses::Status::Archived,
        start(),
        Actor::Instance,
    )
    .await
    .expect("archive");
    f.send((Method::DELETE, &uri), a.id, Some(&f.admin.1), &[])
        .await
        .expect(StatusCode::CONFLICT, "course.archived");
    assert!(f.video_exists(video).await);
}

/// B17: a `video` item whose video is attached to another course only (a row only a direct
/// write could leave) plays nothing: 404 `video.not_found`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_item_never_plays_a_video_outside_its_course(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let item = f.video_item(a.id, a.module, video).await;
    let (other, _) = course(&f.pool, a.id, a.tower, "S2").await;
    sqlx::query("UPDATE course_videos SET course_id = $2 WHERE video_id = $1")
        .bind(video)
        .bind(other)
        .execute(&f.pool)
        .await
        .unwrap();
    f.send(
        (Method::POST, &format!("/api/v1/items/{item}/video-session")),
        a.id,
        Some(&f.admin.1),
        &[],
    )
    .await
    .expect(StatusCode::NOT_FOUND, "video.not_found");
}

/// D46: `videos.purge` deletes nothing of a video that exists, whatever enqueued it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_purge_keeps_a_video_that_exists(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let a = f.a;
    let video = f.ready(a.id, a.course).await;
    let payload = json!({ "video_id": video });
    let job = stepclimb_jobs::NewJob::org(
        &stepclimb_server::video_removal::PURGE,
        scope(a.id),
        &payload,
    )
    .expect("a job");
    let mut tx = f.pool.begin().await.unwrap();
    stepclimb_jobs::enqueue(&mut tx, job, start())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(f.work().await, 1);
    assert_eq!(f.objects(a.id, video).await.len(), OUTPUTS.len());
}
