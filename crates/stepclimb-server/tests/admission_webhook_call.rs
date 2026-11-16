// SPDX-License-Identifier: AGPL-3.0-or-later
//! Step 4 of admission, the admission webhook call, against a receiver on 127.0.0.1
//! (docs/specs/m0/07-admission.md B13 to B17, B19; T1, T2, T10 to T13, T18's last clause, T20
//! and T21 for the webhook). The webhook is created and rotated through its admin routes, so
//! the receiver checks signatures with the secret those routes return, using an HMAC of its own.
//! Sign-ins call `admission::admit` as a provider's callback does. The 3-second limit is tested
//! on the call alone, under tokio's paused time. All data is synthetic; nothing leaves the
//! machine. Needs `DATABASE_URL` (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, KeyInit, Mac};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, webhook::Secret};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider},
    organizations::{self, Tenancy},
    perm,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser, Status, User},
};
use stepclimb_server::{
    AppState,
    admission::{self, Admission, Completion, Context, Profile, Step, Verified},
    admission_webhook::seal_secret,
    admission_webhook_call::{self, Unavailable},
    audit::ClientAddress,
    providers::Providers,
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tokio::net::TcpListener;
use tower::ServiceExt;

mod common;

/// The client address of every request (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const OTHER_KEYS: &str = "other:HyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nzg5Ojs8PT4=";
const WEBHOOK_PATH: &str = "/api/v1/admin/admission/webhook";
/// The path and query of the receiver's URL; neither may reach a log line.
const HOOK_PATH: &str = "/hook-path-x7?tenant=t-query-x7";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the routes that resolve the request's organization"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("an object, not {other}"),
    }
}

/// What the receiver answers.
#[derive(Clone)]
struct Answer {
    status: StatusCode,
    body: Vec<u8>,
    delay: Duration,
}

impl Answer {
    fn json(value: Value) -> Self {
        Self::raw(StatusCode::OK, value.to_string().into_bytes())
    }

    fn raw(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            delay: Duration::ZERO,
        }
    }
}

/// A request the receiver got.
struct Received {
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Received {
    fn header(&self, name: &str) -> &str {
        self.headers[name].to_str().expect("an ASCII header")
    }
}

/// The admission webhook's receiver: records each request and sends the set answer.
#[derive(Clone)]
struct Receiver {
    port: u16,
    received: Arc<Mutex<Vec<Received>>>,
    answer: Arc<Mutex<Answer>>,
}

impl Receiver {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let receiver = Self {
            port: listener.local_addr().expect("an address").port(),
            received: Arc::default(),
            answer: Arc::new(Mutex::new(Answer::json(json!({ "allow": true })))),
        };
        let app = Router::new().fallback(receive).with_state(receiver.clone());
        tokio::spawn(async move { axum::serve(listener, app).await });
        receiver
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}{HOOK_PATH}", self.port)
    }

    fn answer(&self, answer: Answer) {
        *self.answer.lock().expect("answer") = answer;
    }

    fn count(&self) -> usize {
        self.received.lock().expect("received").len()
    }

    /// The last request received.
    fn last<T>(&self, read: impl FnOnce(&Received) -> T) -> T {
        read(
            self.received
                .lock()
                .expect("received")
                .last()
                .expect("a request"),
        )
    }
}

async fn receive(State(receiver): State<Receiver>, headers: HeaderMap, body: Bytes) -> Response {
    receiver.received.lock().expect("received").push(Received {
        headers,
        body: body.to_vec(),
    });
    let answer = receiver.answer.lock().expect("answer").clone();
    tokio::time::sleep(answer.delay).await;
    (answer.status, answer.body).into_response()
}

/// Whether `received` carries a Standard Webhooks signature by `whsec`, checked with an HMAC of
/// the test's own: HMAC-SHA256 over `id.timestamp.body`, keyed with the secret's base64 bytes.
fn verifies(whsec: &str, received: &Received) -> bool {
    let key = STANDARD
        .decode(whsec.strip_prefix("whsec_").expect("the whsec_ prefix"))
        .expect("base64");
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
    mac.update(received.header("webhook-id").as_bytes());
    mac.update(b".");
    mac.update(received.header("webhook-timestamp").as_bytes());
    mac.update(b".");
    mac.update(&received.body);
    let expected = format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()));
    received
        .header("webhook-signature")
        .split(' ')
        .any(|signature| signature == expected)
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
    keys: KeyRing,
    providers: Providers,
    sso: IdentityProvider,
    admin: String,
    receiver: Receiver,
}

impl Fixture {
    /// Org A with an `oidc` provider `sso`, sign-up allowed, an admin holding every
    /// permission, and a receiver; the routes run with the insecure-providers switch, so the
    /// receiver's loopback `http` URL can be saved.
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = org(&pool, "org-a").await;
        let keys = KeyRing::parse(KEYS).expect("a valid test key");
        let providers = Providers::new(true).expect("the outbound clients");
        let state = AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: keys.clone(),
            public_url: WebOrigin::parse("https://lms.example.org").expect("an origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: providers.clone(),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        let sso = provider_in(&pool, org, "sso", json!({})).await;
        allow_signup(&pool, org).await;
        let admin = admin_cookie(&pool, org).await;
        Self {
            pool,
            app,
            org,
            keys,
            providers,
            sso,
            admin,
            receiver: Receiver::start().await,
        }
    }

    /// `PUT` the webhook with `body` as the admin; the answer's JSON.
    async fn put(&self, body: Value) -> Value {
        self.send(Method::PUT, WEBHOOK_PATH, Some(body)).await
    }

    async fn send(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("sec-fetch-site", "same-origin")
            .header(TEST_ORG_HEADER, self.org.to_string())
            .header(header::COOKIE, &self.admin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .expect("a request");
        let response = self.app.clone().oneshot(request).await.expect("an answer");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        serde_json::from_slice(&bytes).expect("JSON")
    }

    /// Create the enabled webhook at the receiver; the secret the route returned.
    async fn webhook(&self) -> String {
        self.webhook_at(&self.receiver.url()).await
    }

    async fn webhook_at(&self, url: &str) -> String {
        let created = self.put(json!({ "url": url, "enabled": true })).await;
        created["secret"].as_str().expect("the secret").to_owned()
    }

    fn context(&self) -> Context<'_> {
        Context {
            scope: scope(self.org),
            client: ClientAddress(CLIENT.ip()),
            now: now(),
            keys: &self.keys,
            providers: &self.providers,
        }
    }

    fn sign_in<'a>(&'a self, subject: &'a str, attrs: Value) -> Verified<'a> {
        Verified {
            provider: &self.sso,
            subject,
            profile: Profile::default(),
            email_verified: false,
            attributes: object(attrs),
        }
    }

    /// Admit `sign_in` under `context` in a transaction that commits whatever admission
    /// answered, as a callback does.
    async fn admit_in(&self, sign_in: &Verified<'_>, context: &Context<'_>) -> Admission {
        let mut tx = self.pool.begin().await.expect("begin");
        let admission = admission::admit(&mut tx, sign_in, context)
            .await
            .expect("admission answers");
        tx.commit().await.expect("commit");
        admission
    }

    async fn outcome(&self, sign_in: &Verified<'_>) -> Option<(Step, &'static str)> {
        match self.admit_in(sign_in, &self.context()).await {
            Admission::Admitted(_) => None,
            Admission::Refused(refusal) => Some((refusal.step(), refusal.code())),
        }
    }

    async fn admitted(&self, sign_in: &Verified<'_>) -> User {
        match self.admit_in(sign_in, &self.context()).await {
            Admission::Admitted(user) => user,
            Admission::Refused(refusal) => panic!("refused: {refusal:?}"),
        }
    }

    /// A user of org A linked to `sso`'s `subject` with `attrs`.
    async fn member(&self, subject: &str, attrs: Value) -> User {
        let user = new_user(&self.pool, self.org, Source::Idp, None).await;
        let identity = NewIdentity {
            user_id: user.id,
            provider_id: self.sso.id,
            subject: subject.to_owned(),
            attributes: object(attrs),
        };
        identities::link(
            &self.pool,
            scope(self.org),
            &identity,
            now(),
            Actor::Instance,
        )
        .await
        .expect("link identity");
        user
    }

    async fn users(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM users WHERE org_id = $1")
            .bind(self.org)
            .fetch_one(&self.pool)
            .await
            .expect("count users")
    }

    async fn marked(&self, user: Uuid) -> bool {
        sqlx::query_scalar(
            "SELECT admission_refused_at IS NOT NULL FROM users WHERE org_id = $1 AND id = $2",
        )
        .bind(self.org)
        .bind(user)
        .fetch_one(&self.pool)
        .await
        .expect("read the marker")
    }

    /// The attributes stored on `sso`'s identity `subject`.
    async fn stored_attributes(&self, subject: &str) -> Value {
        sqlx::query_scalar(
            "SELECT attributes FROM identities
             WHERE org_id = $1 AND provider_id = $2 AND subject = $3",
        )
        .bind(self.org)
        .bind(self.sso.id)
        .bind(subject)
        .fetch_one(&self.pool)
        .await
        .expect("read the identity")
    }

    /// A local sign-in of `user`, which the refusal marker refuses.
    async fn local_sign_in(&self, user: Uuid) -> Option<&'static str> {
        let mut tx = self.pool.begin().await.expect("begin");
        let answer =
            admission::complete_local(&mut tx, scope(self.org), user, now(), &HeaderMap::new())
                .await;
        tx.rollback().await.expect("rollback");
        match answer {
            Ok(_) => None,
            Err(Completion::Refused(refusal)) => Some(refusal.code()),
            Err(Completion::Failed(error)) => panic!("failed: {error:?}"),
        }
    }
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Org", now())
        .await
        .expect("create organization")
        .id
}

async fn provider_in(pool: &PgPool, org: Uuid, key: &str, config: Value) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, $3, 'oidc', $4)",
    )
    .bind(new_id())
    .bind(org)
    .bind(key)
    .bind(config)
    .execute(pool)
    .await
    .expect("insert provider");
    identity_providers::get_by_key(pool, scope(org), key)
        .await
        .expect("read provider")
        .expect("the provider exists")
}

async fn allow_signup(pool: &PgPool, org: Uuid) {
    sqlx::query(
        "UPDATE organizations
         SET settings = settings || '{\"admission\": {\"signup_allowed\": true}}'::jsonb
         WHERE id = $1",
    )
    .bind(org)
    .execute(pool)
    .await
    .expect("allow sign-up");
}

async fn new_user(pool: &PgPool, org: Uuid, source: Source, email: Option<&str>) -> User {
    let new = NewUser {
        display_name: "Pat Example".into(),
        given_name: "Pat".into(),
        family_name: "Example".into(),
        email: email.map(str::to_owned),
        locale: None,
        tz: "UTC".into(),
        source,
        external_id: None,
    };
    users::create(pool, scope(org), &new, now())
        .await
        .expect("create user")
}

/// A session cookie of a new local user of `org` holding every catalog permission.
async fn admin_cookie(pool: &PgPool, org: Uuid) -> String {
    let user = new_user(pool, org, Source::Local, None).await;
    let role = NewRole {
        key: format!("r{}", new_id().simple()),
        name: "Everything".into(),
        permissions: perm::ALL.iter().map(|p| p.key().to_owned()).collect(),
    };
    let role = roles::create_custom(pool, scope(org), &role, now(), Actor::Instance)
        .await
        .expect("create role");
    let assignment = NewAssignment {
        user_id: user.id,
        role_id: role.id,
        scope: Scope::Organization,
        source: Source::Local,
        external_id: None,
    };
    role_assignments::create(pool, scope(org), &assignment, now(), Actor::Instance)
        .await
        .expect("assign");
    let started = session::start(pool, scope(org), user.id, &HeaderMap::new(), now())
        .await
        .expect("a session");
    let cookie = started.set_cookie.to_str().expect("ASCII");
    cookie.split(';').next().expect("a pair").to_owned()
}

const UNAVAILABLE: Option<(Step, &str)> = Some((Step::Webhook, "auth.admission_unavailable"));
const REFUSED: Option<(Step, &str)> = Some((Step::Webhook, "auth.admission_refused"));

/// A log buffer for one test's `tracing` events.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("log").clone()).expect("UTF-8")
    }

    fn clear(&self) {
        self.0.lock().expect("log").clear();
    }

    /// The steps admission ran, in order, from its debug lines.
    fn steps(&self) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| line.contains("admission runs a step"))
            .map(|line| {
                let start = line.find("step=\"").expect("a step field") + 6;
                let rest = &line[start..];
                rest[..rest.find('"').expect("a closing quote")].to_owned()
            })
            .collect()
    }
}

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter("stepclimb_server=debug,stepclimb_auth=debug")
        .with_writer(move || writer.clone())
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

/// T10 (B13, Q7 (a)): the receiver gets a JSON POST of exactly the provider, the subject and the
/// mapped attributes, with a new `webhook-id`, the request clock's time and a signature that
/// verifies with the secret the creating `PUT` returned.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_the_webhook_gets_the_documented_body_signed_with_the_secret(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let secret = f.webhook().await;
    let attrs = json!({ "rating": "S1", "division": "USA" });

    f.admitted(&f.sign_in("sub-new", attrs.clone())).await;
    assert_eq!(f.receiver.count(), 1);
    let first_id = f.receiver.last(|received| {
        let body: Value = serde_json::from_slice(&received.body).expect("a JSON body");
        assert_eq!(
            body,
            json!({ "provider": "sso", "subject": "sub-new", "attributes": attrs })
        );
        // The checker reads a schema-less object as closed: check the shape without members.
        let mut shape = body.clone();
        shape["attributes"] = json!({});
        let document = serde_json::to_value(stepclimb_server::openapi::document()).expect("doc");
        let errors = common::schema_errors(&document, "AdmissionWebhookCall", &shape);
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(received.header("content-type"), "application/json");
        assert_eq!(received.header("webhook-timestamp"), "1763000000");
        assert!(verifies(&secret, received), "the signature verifies");
        received.header("webhook-id").to_owned()
    });
    assert!(
        first_id.starts_with("msg_") && !first_id.contains('.'),
        "{first_id}"
    );

    f.admitted(&f.sign_in("sub-new", json!({}))).await;
    f.receiver.last(|received| {
        assert_ne!(
            received.header("webhook-id"),
            first_id,
            "each call has a new id"
        );
        assert!(verifies(&secret, received));
        let mut forged = Received {
            headers: received.headers.clone(),
            body: received.body.clone(),
        };
        forged.body.push(b' ');
        assert!(
            !verifies(&secret, &forged),
            "a changed body does not verify"
        );
    });
}

/// T18's last clause (B24): after a rotation, calls are signed with the new secret only, so the
/// old secret no longer verifies at the receiver.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_after_rotation_the_old_secret_no_longer_verifies(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let old = f.webhook().await;
    f.admitted(&f.sign_in("sub-1", json!({}))).await;
    assert!(f.receiver.last(|received| verifies(&old, received)));

    let rotated = f
        .send(
            Method::POST,
            "/api/v1/admin/admission/webhook/rotate-secret",
            None,
        )
        .await;
    let new = rotated["secret"].as_str().expect("a secret").to_owned();
    assert_ne!(new, old);
    f.admitted(&f.sign_in("sub-1", json!({}))).await;
    f.receiver.last(|received| {
        assert!(
            !verifies(&old, received),
            "the old secret no longer verifies"
        );
        assert!(verifies(&new, received), "the new one does");
    });
}

/// T13 (B15, B3) and the refusal marker: `allow: false` refuses with `auth.admission_refused`;
/// a new person gets no user, and a known user gets the marker, so a local sign-in is refused
/// too, until a sign-in the webhook allows clears it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_allow_false_refuses_rolls_back_and_marks_a_known_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let pat = f.member("sub-pat", json!({ "rating": "S1" })).await;
    let users = f.users().await;
    f.receiver.answer(Answer::json(json!({ "allow": false })));

    assert_eq!(f.outcome(&f.sign_in("sub-new", json!({}))).await, REFUSED);
    assert_eq!(f.users().await, users, "no user is created");

    let attrs = json!({ "rating": "C1" });
    assert_eq!(
        f.outcome(&f.sign_in("sub-pat", attrs.clone())).await,
        REFUSED
    );
    assert_eq!(
        f.stored_attributes("sub-pat").await,
        json!({ "rating": "S1" })
    );
    assert!(f.marked(pat.id).await);
    assert_eq!(
        f.local_sign_in(pat.id).await,
        Some("auth.admission_refused")
    );

    f.receiver.answer(Answer::json(json!({ "allow": true })));
    f.admitted(&f.sign_in("sub-pat", attrs.clone())).await;
    assert!(
        !f.marked(pat.id).await,
        "a sign-in the webhook allows clears it"
    );
    assert_eq!(f.stored_attributes("sub-pat").await, attrs);
    assert_eq!(f.local_sign_in(pat.id).await, None);
}

/// B14: a webhook that cannot decide refuses with `auth.admission_unavailable` and records no
/// marker; a marker already set stays, since the sign-in's writes are rolled back.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unavailable_webhook_refuses_and_leaves_the_marker_as_it_was(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let pat = f.member("sub-pat", json!({})).await;
    f.receiver
        .answer(Answer::raw(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()));

    assert_eq!(
        f.outcome(&f.sign_in("sub-pat", json!({}))).await,
        UNAVAILABLE
    );
    assert!(!f.marked(pat.id).await, "an outage marks no one");
    assert_eq!(f.local_sign_in(pat.id).await, None);

    f.receiver.answer(Answer::json(json!({ "allow": false })));
    assert_eq!(f.outcome(&f.sign_in("sub-pat", json!({}))).await, REFUSED);
    f.receiver
        .answer(Answer::raw(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()));
    assert_eq!(
        f.outcome(&f.sign_in("sub-pat", json!({}))).await,
        UNAVAILABLE
    );
    assert!(f.marked(pat.id).await, "an outage clears no marker");
}

/// T11 (B14, B16): each failure but the timeout refuses a new person with
/// `auth.admission_unavailable` and creates no user (the timeout is tested on the call, under
/// paused time). Nothing is retried: each sign-in calls once.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t11_every_failure_refuses_with_admission_unavailable(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let users = f.users().await;
    let mut big = br#"{"allow": true, "pad": ""#.to_vec();
    big.resize(65 * 1024, b'a');
    big.extend_from_slice(br#""}"#);
    let failures = [
        (
            "500",
            Answer::raw(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
        ),
        ("302", Answer::raw(StatusCode::FOUND, Vec::new())),
        (
            "201",
            Answer::raw(StatusCode::CREATED, br#"{"allow": true}"#.to_vec()),
        ),
        ("not JSON", Answer::raw(StatusCode::OK, b"allow".to_vec())),
        ("allow missing", Answer::json(json!({ "groups": [] }))),
        ("allow a string", Answer::json(json!({ "allow": "true" }))),
        (
            "groups a string",
            Answer::json(json!({ "allow": true, "groups": "staff" })),
        ),
        (
            "attributes an array",
            Answer::json(json!({ "allow": true, "attributes": [] })),
        ),
        ("65 KiB", Answer::raw(StatusCode::OK, big)),
    ];
    for (calls, (name, answer)) in (1..).zip(failures) {
        f.receiver.answer(answer);
        let outcome = f.outcome(&f.sign_in("sub-new", json!({}))).await;
        assert_eq!(outcome, UNAVAILABLE, "{name}");
        assert_eq!(f.receiver.count(), calls, "{name}: one call, no retry");
    }
    assert_eq!(f.users().await, users);
}

/// T11 (B14): connection refused fails closed, and neither the warning nor any other log line
/// carries the URL's host, path or query.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_connection_fails_closed_and_the_url_is_never_logged(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = closed.local_addr().expect("an address").port();
    drop(closed);
    f.webhook_at(&format!("http://127.0.0.1:{port}{HOOK_PATH}"))
        .await;
    let (captured, _guard) = capture();

    assert_eq!(
        f.outcome(&f.sign_in("sub-new", json!({}))).await,
        UNAVAILABLE
    );
    let log = captured.text();
    let warning = log
        .lines()
        .find(|line| line.contains("could not decide"))
        .expect("a warning");
    assert!(warning.contains("provider=\"sso\""), "{warning}");
    assert!(warning.contains("request failed"), "{warning}");
    for part in [
        format!(":{port}"),
        "hook-path-x7".into(),
        "t-query-x7".into(),
    ] {
        assert!(!log.contains(&part), "{part} in {log}");
    }

    f.put(json!({ "url": f.receiver.url() })).await;
    f.receiver.answer(Answer::raw(
        StatusCode::BAD_GATEWAY,
        b"hook-path-x7".to_vec(),
    ));
    captured.clear();
    assert_eq!(
        f.outcome(&f.sign_in("sub-new", json!({}))).await,
        UNAVAILABLE
    );
    let log = captured.text();
    assert!(log.contains("status 502"), "{log}");
    for part in [
        format!(":{}", f.receiver.port),
        "hook-path-x7".into(),
        "t-query-x7".into(),
    ] {
        assert!(!log.contains(&part), "{part} in {log}");
    }
}

/// B19 (T14's attribute half): the reply's `attributes` are merged over the mapped ones, its
/// keys winning, and stored on a known user's identity and a new user's alike.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_webhooks_attributes_are_merged_over_the_mapped_ones(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    f.member("sub-pat", json!({ "rating": "S1" })).await;
    f.receiver.answer(Answer::json(json!({
        "allow": true,
        "groups": ["staff"],
        "attributes": { "rating": "C1", "roster": "home" },
    })));
    let mapped = json!({ "rating": "S2", "division": "USA" });
    let merged = json!({ "rating": "C1", "division": "USA", "roster": "home" });

    f.admitted(&f.sign_in("sub-pat", mapped.clone())).await;
    assert_eq!(f.stored_attributes("sub-pat").await, merged);
    f.admitted(&f.sign_in("sub-new", mapped.clone())).await;
    assert_eq!(f.stored_attributes("sub-new").await, merged);
    let sent: Value = f
        .receiver
        .last(|received| serde_json::from_slice(&received.body).expect("JSON"));
    assert_eq!(
        sent["attributes"], mapped,
        "the webhook gets the mapped attributes"
    );
    let document = serde_json::to_value(stepclimb_server::openapi::document()).expect("doc");
    let reply = json!({ "allow": true, "groups": ["staff"], "attributes": {} });
    let errors = common::schema_errors(&document, "AdmissionWebhookReply", &reply);
    assert!(errors.is_empty(), "{errors:#?}");
}

/// B13: a disabled webhook is skipped, and so is a deleted one; the receiver gets nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_disabled_or_missing_webhook_is_skipped(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    f.receiver.answer(Answer::json(json!({ "allow": false })));
    f.put(json!({ "enabled": false })).await;
    f.admitted(&f.sign_in("sub-1", json!({}))).await;

    let request = Request::builder()
        .method(Method::DELETE)
        .uri(WEBHOOK_PATH)
        .header("sec-fetch-site", "same-origin")
        .header(TEST_ORG_HEADER, f.org.to_string())
        .header(header::COOKIE, &f.admin)
        .body(Body::empty())
        .expect("a request");
    let deleted = f.app.clone().oneshot(request).await.expect("an answer");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    f.admitted(&f.sign_in("sub-2", json!({}))).await;
    assert_eq!(f.receiver.count(), 0);
}

/// Permissions (spec 07 I6): an organization whose plan lost `admission_webhook` still
/// has its enabled webhook called, so the gate it configured keeps refusing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_webhook_is_called_without_the_plan_feature(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let features: Map<String, Value> = stepclimb_core::feature::ALL
        .iter()
        .map(|key| ((*key).to_owned(), Value::Bool(*key != "admission_webhook")))
        .collect();
    let limits: Map<String, Value> = stepclimb_core::feature::Limit::ALL
        .iter()
        .map(|limit| (limit.key().to_owned(), Value::Null))
        .collect();
    let plan = new_id();
    sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3, $4)")
        .bind(plan)
        .bind(format!("test_{}", plan.simple()))
        .bind(Value::Object(features))
        .bind(Value::Object(limits))
        .execute(&f.pool)
        .await
        .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(f.org)
        .bind(plan)
        .execute(&f.pool)
        .await
        .expect("move to the plan");
    f.receiver.answer(Answer::json(json!({ "allow": false })));

    assert_eq!(f.outcome(&f.sign_in("sub-1", json!({}))).await, REFUSED);
    assert_eq!(f.receiver.count(), 1);
}

/// B17 (spec 07 I6): the URL is checked against the policy of the instance as it is at
/// the call. A private address saved on a one-organization instance is refused, before
/// anything is sent, once a second organization exists.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_private_address_is_refused_once_the_instance_has_two_organizations(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook_at("https://[fd00::5]/hook").await;
    org(&f.pool, "org-b").await;
    let (captured, _guard) = capture();

    assert_eq!(f.outcome(&f.sign_in("sub-1", json!({}))).await, UNAVAILABLE);
    let log = captured.text();
    assert!(log.contains("policy refused"), "{log}");
    assert!(log.contains("private"), "{log}");
    assert!(!log.contains("fd00"), "{log}");
}

/// B17 (spec 07 I6): a host name is checked at each call by the client's resolver, so a
/// name that resolves to a loopback address is refused without the insecure switch, and nothing
/// reaches the receiver.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_name_resolving_to_loopback_is_refused_at_the_call(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let port = f.receiver.port;
    f.webhook_at(&format!("https://localhost:{port}/hook"))
        .await;
    let strict = Providers::new(false).expect("the outbound clients");
    let context = Context {
        providers: &strict,
        ..f.context()
    };
    let (captured, _guard) = capture();

    let outcome = match f.admit_in(&f.sign_in("sub-1", json!({})), &context).await {
        Admission::Refused(refusal) => (refusal.step(), refusal.code()),
        Admission::Admitted(user) => panic!("admitted {}", user.id),
    };
    assert_eq!(Some(outcome), UNAVAILABLE);
    let log = captured.text();
    assert!(log.contains("policy refused"), "{log}");
    assert!(log.contains("loopback"), "{log}");
    assert_eq!(f.receiver.count(), 0);
}

/// B14 (spec 07 I6): a secret that does not open fails closed, without a call: another key
/// ring, or a value sealed for another row.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_secret_that_does_not_open_fails_closed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let other = KeyRing::parse(OTHER_KEYS).expect("a valid test key");
    let context = Context {
        keys: &other,
        ..f.context()
    };
    let outcome = match f.admit_in(&f.sign_in("sub-1", json!({})), &context).await {
        Admission::Refused(refusal) => Some((refusal.step(), refusal.code())),
        Admission::Admitted(_) => None,
    };
    assert_eq!(outcome, UNAVAILABLE);

    let secret = Secret::generate().expect("a secret");
    let sealed = seal_secret(&f.keys, scope(f.org), new_id(), &secret).expect("seal");
    sqlx::query("UPDATE admission_webhooks SET secret = $2 WHERE org_id = $1")
        .bind(f.org)
        .bind(sealed)
        .execute(&f.pool)
        .await
        .expect("store another row's secret");
    assert_eq!(f.outcome(&f.sign_in("sub-1", json!({}))).await, UNAVAILABLE);
    assert_eq!(f.receiver.count(), 0);
}

/// T1 (B4): a suspended known user is refused at step 1 and the webhook is never called.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_suspended_user_never_reaches_the_webhook(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let pat = f.member("sub-pat", json!({})).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        pat.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");

    assert_eq!(
        f.outcome(&f.sign_in("sub-pat", json!({}))).await,
        Some((Step::KnownIdentity, "auth.suspended"))
    );
    assert_eq!(f.receiver.count(), 0);
}

/// T2 (B3, B5): a refusal at step 4 after an email match leaves no identity behind, and marks
/// the matched user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_webhook_refusal_after_an_email_match_links_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    let trusted = provider_in(
        &f.pool,
        f.org,
        "trusted",
        json!({ "trusted_for_email_matching": true }),
    )
    .await;
    sqlx::query(
        "UPDATE organizations SET settings = settings || '{\"email_matching\": true}'::jsonb
         WHERE id = $1",
    )
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("turn on email matching");
    let robin = new_user(&f.pool, f.org, Source::Local, Some("robin@example.test")).await;
    let sign_in = Verified {
        provider: &trusted,
        subject: "sub-robin",
        profile: Profile {
            email: Some("robin@example.test".into()),
            ..Profile::default()
        },
        email_verified: true,
        attributes: Map::new(),
    };
    f.receiver.answer(Answer::json(json!({ "allow": false })));

    assert_eq!(f.outcome(&sign_in).await, REFUSED);
    let linked = identities::list_for_user(&f.pool, scope(f.org), robin.id)
        .await
        .expect("list identities");
    assert!(linked.is_empty(), "{linked:?}");
    assert!(f.marked(robin.id).await);

    f.receiver.answer(Answer::json(json!({ "allow": true })));
    assert_eq!(f.admitted(&sign_in).await.id, robin.id);
}

/// T20: org A's webhook is never called for org B's sign-ins.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_webhook_is_never_called(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    f.receiver.answer(Answer::json(json!({ "allow": false })));
    let b = org(&f.pool, "org-b").await;
    allow_signup(&f.pool, b).await;
    let sso_b = provider_in(&f.pool, b, "sso", json!({})).await;
    let sign_in = Verified {
        provider: &sso_b,
        ..f.sign_in("sub-b", json!({}))
    };
    let context = Context {
        scope: scope(b),
        ..f.context()
    };

    assert!(matches!(
        f.admit_in(&sign_in, &context).await,
        Admission::Admitted(_)
    ));
    assert_eq!(f.receiver.count(), 0);
}

/// T21 (B1, every step): a new person runs steps 1 to 5 in order, a known user steps 1, 2 and
/// 4, and a refusal by the webhook runs no step after it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t21_the_webhook_runs_fourth_and_stops_the_steps_when_it_refuses(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    sqlx::query(
        "UPDATE organizations
         SET settings = jsonb_set(settings, '{admission,rules}',
                                  '[{\"attribute\": \"division\", \"op\": \"equals\", \"value\": \"USA\"}]')
         WHERE id = $1",
    )
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("store a rule");
    f.member("sub-pat", json!({})).await;
    let usa = json!({ "division": "USA" });
    let (captured, _guard) = capture();

    f.admitted(&f.sign_in("sub-new", usa.clone())).await;
    assert_eq!(
        captured.steps(),
        [
            "known_identity",
            "rules",
            "sync_only",
            "webhook",
            "just_in_time"
        ]
    );
    captured.clear();
    f.admitted(&f.sign_in("sub-pat", usa.clone())).await;
    assert_eq!(captured.steps(), ["known_identity", "rules", "webhook"]);

    f.receiver.answer(Answer::json(json!({ "allow": false })));
    captured.clear();
    assert_eq!(f.outcome(&f.sign_in("sub-new-2", usa)).await, REFUSED);
    assert_eq!(
        captured.steps(),
        ["known_identity", "rules", "sync_only", "webhook"]
    );
}

/// A receiver and the admission webhook client of a one-organization instance with the
/// insecure switch, for the timing tests.
async fn timed_call(delay: Duration) -> Result<admission_webhook_call::Reply, Unavailable> {
    let receiver = Receiver::start().await;
    let mut answer = Answer::json(json!({ "allow": true }));
    answer.delay = delay;
    receiver.answer(answer);
    let providers = Providers::new(true).expect("the outbound clients");
    let client = providers.admission_webhook(Tenancy::Single(new_id()));
    let request =
        admission_webhook_call::request(&receiver.url(), "msg_1", 1, "v1,", b"{}".to_vec())
            .expect("a request");
    admission_webhook_call::send(client, request).await
}

/// T12 (B14): a reply 2.9 s after the call started is accepted.
#[tokio::test(start_paused = true)]
async fn t12_a_reply_at_2_9_seconds_is_in_time() {
    let reply = timed_call(Duration::from_millis(2_900))
        .await
        .expect("in time");
    assert!(reply.allow);
}

/// T11 (B14): a reply just after 3.0 s is a timeout.
#[tokio::test(start_paused = true)]
async fn t11_a_reply_after_3_seconds_is_a_timeout() {
    let late = timed_call(Duration::from_millis(3_001)).await;
    assert!(
        matches!(
            late,
            Err(Unavailable::Outbound(
                stepclimb_auth::outbound::OutboundError::Timeout(limit)
            )) if limit == Duration::from_secs(3)
        ),
        "{late:?}"
    );
}

/// Q3 (a): only sign-ins through an outside provider call the webhook. A sign-in through the
/// local provider is admitted without a call, even with a webhook that refuses everyone.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_sign_in_never_calls_the_webhook(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.webhook().await;
    f.receiver.answer(Answer::json(json!({ "allow": false })));
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'local', 'local', '{}'::jsonb, true)
         ON CONFLICT (org_id, key) DO NOTHING",
    )
    .bind(new_id())
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("a local row");
    let local = identity_providers::get_by_key(&f.pool, scope(f.org), "local")
        .await
        .expect("read provider")
        .expect("the local row exists");
    let pat = new_user(&f.pool, f.org, Source::Local, None).await;
    let subject = pat.id.to_string();
    let identity = NewIdentity {
        user_id: pat.id,
        provider_id: local.id,
        subject: subject.clone(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link identity");
    let sign_in = Verified {
        provider: &local,
        ..f.sign_in(&subject, json!({}))
    };

    assert_eq!(f.admitted(&sign_in).await.id, pat.id);
    assert_eq!(f.local_sign_in(pat.id).await, None);
    assert_eq!(f.receiver.count(), 0);
}
