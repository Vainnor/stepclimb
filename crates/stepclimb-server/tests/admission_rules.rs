// SPDX-License-Identifier: AGPL-3.0-or-later
//! Admission rules at sign-in and over `/api/v1/admin/admission` (docs/specs/m0/07-admission.md
//! B8 to B10, Q1 to Q3; T7, T8, T21 for steps 1, 2 and 5; T19 and T20 for the rules). The
//! permission suite's four cases and cross-organization case for both operations are in
//! `tests/permissions.rs`; these tests cover what the rules decide at sign-in, in which order the
//! steps run, the fail-closed paths, and what the route stores, refuses and audits.
//!
//! Sign-ins call `admission::admit` as a provider's callback does. Providers are inserted by SQL
//! with a config sealed by `stepclimb_auth`, since the provider routes are spec 06's. Requests
//! name their organization with the `test-support` header of `request_org`. All data is
//! synthetic. Needs `DATABASE_URL` (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_auth::{
    encryption::KeyRing, oauth2_config, oidc_config, outbound::UrlPolicy,
    provider_config::ProviderRow,
};
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider},
    organizations,
    perm::{self, AdmissionManage, Permission},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser, User},
};
use stepclimb_server::providers::Providers;
use stepclimb_server::{
    AppState,
    admission::{self, Admission, Context, Profile, Step, Verified},
    audit::ClientAddress,
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address of every request and sign-in (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const PATH: &str = "/api/v1/admin/admission";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

/// The key ring and outbound clients admission's step 4 is given; no organization in these
/// tests has an admission webhook, so neither is used.
static KEY_RING: LazyLock<KeyRing> = LazyLock::new(|| {
    KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").expect("a valid test key")
});
static PROVIDERS: LazyLock<Providers> =
    LazyLock::new(|| Providers::new(false).expect("the outbound clients"));

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn keys() -> KeyRing {
    KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").expect("a valid test key")
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("an object, not {other}"),
    }
}

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

/// The rule T7 names: attribute `subdivision` equals `USA`.
fn usa() -> Value {
    json!({ "attribute": "subdivision", "op": "equals", "value": "USA" })
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    fn errors(&self) -> Vec<(String, String)> {
        self.body["errors"]
            .as_array()
            .expect("errors")
            .iter()
            .map(|e| {
                let field = e["field"].as_str().expect("a field").to_owned();
                (field, e["code"].as_str().expect("a code").to_owned())
            })
            .collect()
    }
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
    /// An `oidc` provider mapping the attributes `subdivision` and `rating`.
    sso: IdentityProvider,
    /// An `oauth2` provider mapping the attribute `region`.
    club: IdentityProvider,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = org(&pool, "org-a").await;
        let sso = oidc_provider(&pool, org, "sso").await;
        let club = oauth2_provider(&pool, org, "club").await;
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: keys(),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self {
            pool,
            app,
            org,
            sso,
            club,
        }
    }

    /// A signed-in user of the organization holding a custom role with `keys` at organization
    /// scope: the `Cookie` value.
    async fn holder(&self, keys: &[&str]) -> String {
        let user = user_in(&self.pool, self.org).await.id;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Holder".into(),
            permissions: keys.iter().map(|k| (*k).to_owned()).collect(),
        };
        let role = roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id;
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: Scope::Organization,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.org),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
        let started = session::start(&self.pool, scope(self.org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    /// A holder of every catalog permission.
    async fn admin(&self) -> String {
        let all: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
        self.holder(&all).await
    }

    async fn send(&self, method: Method, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(PATH)
            .header("sec-fetch-site", "same-origin")
            .header(TEST_ORG_HEADER, self.org.to_string())
            .header(header::COOKIE, cookie);
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("a JSON body")
        };
        Answer { status, body }
    }

    async fn put(&self, cookie: &str, body: Value) -> Answer {
        self.send(Method::PUT, cookie, Some(body)).await
    }

    /// Everything a rules write could touch: the organization's settings and `updated_at`, each
    /// provider's config and `updated_at`, and the number of audit entries.
    async fn snapshot(&self) -> Value {
        sqlx::query_scalar(
            "SELECT jsonb_build_object(
                 'settings', o.settings, 'updated_at', o.updated_at::text,
                 'providers', (SELECT jsonb_agg(jsonb_build_array(p.key, p.config,
                                                                  p.updated_at::text)
                                                ORDER BY p.key)
                               FROM identity_providers p WHERE p.org_id = o.id),
                 'audit', (SELECT count(*) FROM audit_events a WHERE a.org_id = o.id))
             FROM organizations o WHERE o.id = $1",
        )
        .bind(self.org)
        .fetch_one(&self.pool)
        .await
        .expect("read the organization")
    }

    async fn allow_signup(&self) {
        allow_signup(&self.pool, self.org).await;
    }
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, "Org", now())
        .await
        .expect("create organization")
        .id
}

async fn allow_signup(pool: &PgPool, org: Uuid) {
    sqlx::query(
        r#"UPDATE organizations
           SET settings = settings || '{"admission": {"signup_allowed": true}}'::jsonb
           WHERE id = $1"#,
    )
    .bind(org)
    .execute(pool)
    .await
    .expect("allow sign-up");
}

async fn insert_provider(
    pool: &PgPool,
    org: Uuid,
    id: Uuid,
    key: &str,
    kind: &str,
    config: Map<String, Value>,
) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, $3, $4, $5, true)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .bind(kind)
    .bind(Value::Object(config))
    .execute(pool)
    .await
    .expect("insert provider");
    identity_providers::get_by_key(pool, scope(org), key)
        .await
        .expect("read provider")
        .expect("the provider exists")
}

/// An `oidc` provider of `org` mapping `subdivision` and `rating`.
async fn oidc_provider(pool: &PgPool, org: Uuid, key: &str) -> IdentityProvider {
    let id = new_id();
    let config = json!({
        "issuer": "https://idp.example.test",
        "client_id": "stepclimb",
        "client_secret": "client-secret-for-tests",
        "claims": { "attributes": { "subdivision": "/division", "rating": "/rating" } },
    });
    let row = ProviderRow {
        org_id: org,
        provider_id: id,
    };
    let stored = oidc_config::validate(&config, UrlPolicy::new(true, true), None)
        .expect("a valid config")
        .seal(&keys(), row)
        .expect("seal")
        .to_json();
    insert_provider(pool, org, id, key, "oidc", stored).await
}

/// An `oauth2` provider of `org` mapping `region`.
async fn oauth2_provider(pool: &PgPool, org: Uuid, key: &str) -> IdentityProvider {
    let id = new_id();
    let config = json!({
        "authorize": "https://club.example.test/authorize",
        "token": "https://club.example.test/token",
        "userinfo": "https://club.example.test/userinfo",
        "client_id": "stepclimb",
        "client_secret": "client-secret-for-tests",
        "claims": { "subject": "/id", "attributes": { "region": "/region" } },
    });
    let row = ProviderRow {
        org_id: org,
        provider_id: id,
    };
    let stored = oauth2_config::validate(&config, UrlPolicy::new(true, true), None)
        .expect("a valid config")
        .seal(&keys(), row)
        .expect("seal")
        .to_json();
    insert_provider(pool, org, id, key, "oauth2", stored).await
}

/// The organization's local provider row, inserted when it has none yet.
async fn local_provider(pool: &PgPool, org: Uuid) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'local', 'local', '{}'::jsonb, true)
         ON CONFLICT (org_id, key) DO NOTHING",
    )
    .bind(new_id())
    .bind(org)
    .execute(pool)
    .await
    .expect("a local row");
    identity_providers::get_by_key(pool, scope(org), "local")
        .await
        .expect("read provider")
        .expect("the local row exists")
}

async fn user_in(pool: &PgPool, org: Uuid) -> User {
    let new = NewUser {
        display_name: "Pat Example".into(),
        given_name: "Pat".into(),
        family_name: "Example".into(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Idp,
        external_id: None,
    };
    users::create(pool, scope(org), &new, now())
        .await
        .expect("create user")
}

/// A user of `org` linked to `provider`'s `subject` with the stored `attrs`.
async fn member(
    pool: &PgPool,
    org: Uuid,
    provider: &IdentityProvider,
    subject: &str,
    attrs: Value,
) -> User {
    let user = user_in(pool, org).await;
    let identity = NewIdentity {
        user_id: user.id,
        provider_id: provider.id,
        subject: subject.to_owned(),
        attributes: object(attrs),
    };
    identities::link(pool, scope(org), &identity, now(), Actor::Instance)
        .await
        .expect("link identity");
    user
}

/// Store `rules` as the organization's list, by SQL.
async fn store_org_rules(pool: &PgPool, org: Uuid, rules: Value) {
    sqlx::query(
        "UPDATE organizations
         SET settings = settings || jsonb_build_object('admission',
             COALESCE(settings -> 'admission', '{}'::jsonb) || jsonb_build_object('rules', $2::jsonb))
         WHERE id = $1",
    )
    .bind(org)
    .bind(rules)
    .execute(pool)
    .await
    .expect("store organization rules");
}

/// Store `rules` as `provider`'s list, by SQL; returns the provider as stored.
async fn store_provider_rules(
    pool: &PgPool,
    org: Uuid,
    provider: &IdentityProvider,
    rules: Value,
) -> IdentityProvider {
    sqlx::query(
        "UPDATE identity_providers
         SET config = config || jsonb_build_object('admission_rules', $3::jsonb)
         WHERE org_id = $1 AND id = $2",
    )
    .bind(org)
    .bind(provider.id)
    .bind(rules)
    .execute(pool)
    .await
    .expect("store provider rules");
    identity_providers::get_by_key(pool, scope(org), &provider.key)
        .await
        .expect("read provider")
        .expect("the provider exists")
}

/// Admit a sign-in through `provider` of `org` as `subject` with `attrs`, in a transaction that
/// commits whatever admission answered, as a callback that deletes its flow does.
async fn admit(
    pool: &PgPool,
    org: Uuid,
    provider: &IdentityProvider,
    subject: &str,
    attrs: Value,
) -> Result<Admission, stepclimb_server::error::ApiError> {
    let sign_in = Verified {
        provider,
        subject,
        profile: Profile::default(),
        email_verified: false,
        attributes: object(attrs),
    };
    let context = Context {
        scope: scope(org),
        client: ClientAddress(CLIENT.ip()),
        now: now(),
        keys: &KEY_RING,
        providers: &PROVIDERS,
    };
    let mut tx = pool.begin().await.expect("begin");
    let admission = admission::admit(&mut tx, &sign_in, &context).await;
    tx.commit().await.expect("commit");
    admission
}

/// The step and code of a refusal, or `None` for an admitted sign-in.
async fn refusal(
    pool: &PgPool,
    org: Uuid,
    provider: &IdentityProvider,
    subject: &str,
    attrs: Value,
) -> Option<(Step, &'static str)> {
    match admit(pool, org, provider, subject, attrs)
        .await
        .expect("admission answers")
    {
        Admission::Admitted(_) => None,
        Admission::Refused(refusal) => Some((refusal.step(), refusal.code())),
    }
}

const REFUSED: Option<(Step, &str)> = Some((Step::Rules, "auth.admission_refused"));

async fn count_users(pool: &PgPool, org: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("count users")
}

/// A log buffer for one test's `tracing` events.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("log").clone()).expect("UTF-8")
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

    fn clear(&self) {
        self.0.lock().expect("log").clear();
    }
}

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
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
        .with_env_filter("stepclimb_server=debug")
        .with_writer(move || writer.clone())
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

/// T7 (B8, B9): a rule requiring `subdivision` to equal `USA` admits a matching new person,
/// refuses another value and refuses when the attribute is absent, creating no user for either.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_rule_admits_a_match_and_refuses_another_value_or_an_absent_attribute(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;

    let other = json!({ "subdivision": "CAN" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-can", other).await,
        REFUSED
    );
    let absent = json!({ "rating": 5 });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-none", absent).await,
        REFUSED
    );
    assert_eq!(
        count_users(&f.pool, f.org).await,
        0,
        "a refusal creates no user"
    );

    let matching = json!({ "subdivision": "USA" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-usa", matching).await,
        None
    );
    assert_eq!(count_users(&f.pool, f.org).await, 1);
}

/// T8 (B10, Q3 (a)): rules run on every sign-in. A known user who no longer meets a rule is
/// refused, and the refusal leaves the stored attributes as they were; once they meet it again
/// they are admitted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_a_known_user_who_no_longer_meets_a_rule_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = member(
        &f.pool,
        f.org,
        &f.sso,
        "sub-pat",
        json!({ "subdivision": "USA" }),
    )
    .await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;

    let moved = json!({ "subdivision": "CAN" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", moved).await,
        REFUSED
    );
    let stored = identities::list_for_user(&f.pool, scope(f.org), pat.id)
        .await
        .expect("identities");
    assert_eq!(
        stored[0].attributes,
        object(json!({ "subdivision": "USA" }))
    );

    let back = json!({ "subdivision": "USA", "rating": 3 });
    let admitted = admit(&f.pool, f.org, &f.sso, "sub-pat", back)
        .await
        .expect("answers");
    assert!(matches!(admitted, Admission::Admitted(user) if user.id == pat.id));
}

/// B9: a rule that cannot be evaluated (the attribute has the wrong type) refuses, as does a
/// group holding one, even an `any` whose other rule holds.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unevaluable_rule_refuses(pool: PgPool) {
    let f = Fixture::new(pool).await;
    member(&f.pool, f.org, &f.sso, "sub-pat", json!({})).await;
    let rated = json!({ "attribute": "rating", "op": "equals", "value": 5 });
    store_org_rules(&f.pool, f.org, json!([rated])).await;
    let text = json!({ "rating": "5" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", text).await,
        REFUSED
    );
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", json!({ "rating": 5 })).await,
        None
    );

    store_org_rules(&f.pool, f.org, json!([{ "any": [usa(), rated] }])).await;
    let mixed = json!({ "subdivision": "USA", "rating": "5" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", mixed).await,
        REFUSED
    );
}

/// Q2 (a): a provider's list applies to that provider's sign-ins only; the organization's list
/// applies to every outside provider's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_providers_list_applies_to_its_own_sign_ins_and_the_organizations_to_all(pool: PgPool) {
    let f = Fixture::new(pool).await;
    member(&f.pool, f.org, &f.sso, "sub-sso", json!({})).await;
    member(&f.pool, f.org, &f.club, "sub-club", json!({})).await;
    let sso = store_provider_rules(&f.pool, f.org, &f.sso, json!([usa()])).await;

    assert_eq!(
        refusal(&f.pool, f.org, &sso, "sub-sso", json!({})).await,
        REFUSED
    );
    assert_eq!(
        refusal(&f.pool, f.org, &f.club, "sub-club", json!({})).await,
        None
    );

    let region = json!({ "attribute": "region", "op": "exists" });
    store_org_rules(&f.pool, f.org, json!([region])).await;
    let usa_only = json!({ "subdivision": "USA" });
    assert_eq!(
        refusal(&f.pool, f.org, &sso, "sub-sso", usa_only).await,
        REFUSED
    );
    let both = json!({ "subdivision": "USA", "region": "east" });
    assert_eq!(refusal(&f.pool, f.org, &sso, "sub-sso", both).await, None);
    let club = json!({ "region": "east" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.club, "sub-club", club).await,
        None
    );
    assert_eq!(
        refusal(&f.pool, f.org, &f.club, "sub-club", json!({})).await,
        REFUSED
    );
}

/// T21 (B1, steps 1, 2, 3 and 5): with rules and sign-up configured, a new person runs steps 1,
/// 2, 3 and 5 in that order; a refusal at step 2 runs no later step, and a refusal at step 1 (a
/// suspended user) runs no step 2.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t21_the_steps_run_in_order_and_stop_at_a_refusal(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let pat = member(&f.pool, f.org, &f.sso, "sub-pat", json!({})).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        pat.id,
        users::Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    let (captured, _guard) = capture();

    let usa_attrs = json!({ "subdivision": "USA" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-new", usa_attrs.clone()).await,
        None
    );
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
    let other = json!({ "subdivision": "CAN" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-new-2", other).await,
        REFUSED
    );
    assert_eq!(captured.steps(), ["known_identity", "rules"]);

    captured.clear();
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", usa_attrs).await,
        Some((Step::KnownIdentity, "auth.suspended"))
    );
    assert_eq!(captured.steps(), ["known_identity"]);
}

/// B2, T17 for step 2: the refusal redirects with only its code, and its log line names the
/// provider, the step, the rule's id and outcome, and holds no subject or attribute value.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_rule_refusal_logs_the_rule_and_no_attribute_value(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let rated = json!({ "attribute": "rating", "op": "in", "value": [4, 5] });
    let sso = store_provider_rules(&f.pool, f.org, &f.sso, json!([usa(), rated])).await;
    member(&f.pool, f.org, &sso, "sub-secret-pat", json!({})).await;
    let (captured, _guard) = capture();

    let attrs = json!({ "subdivision": "USA", "rating": "value-S3" });
    let Admission::Refused(refused) = admit(&f.pool, f.org, &sso, "sub-secret-pat", attrs)
        .await
        .expect("answers")
    else {
        panic!("admitted")
    };
    assert_eq!(
        refused.redirect().headers()[header::LOCATION],
        "/sign-in?error=auth.admission_refused"
    );
    let problem = refused.into_error().problem();
    assert_eq!(
        (problem.status, problem.code.as_str()),
        (403, "auth.admission_refused")
    );

    let log = captured.text();
    let line = log
        .lines()
        .find(|line| line.contains("admission refused"))
        .expect("a refusal line");
    for part in [
        "provider=\"sso\"",
        "step=\"rules\"",
        "code=\"auth.admission_refused\"",
        "rule=\"/rules/providers/sso/1\"",
        "outcome=\"unevaluable\"",
    ] {
        assert!(line.contains(part), "{part} in {line}");
    }
    for secret in ["sub-secret", "value-S3", "USA"] {
        assert!(!log.contains(secret), "{secret} in {log}");
    }
}

/// Fail closed: a stored list that does not parse (a newer release's form, corruption) admits
/// no one through the providers it applies to, and leaves nothing behind.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_list_that_does_not_parse_admits_no_one(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    member(&f.pool, f.org, &f.club, "sub-club", json!({})).await;
    let newer = json!([{ "attribute": "subdivision", "op": "starts_with", "value": "U" }]);
    let sso = store_provider_rules(&f.pool, f.org, &f.sso, newer).await;
    let failed = admit(
        &f.pool,
        f.org,
        &sso,
        "sub-new",
        json!({ "subdivision": "USA" }),
    )
    .await;
    assert_eq!(failed.expect_err("a fault").problem().status, 500);
    assert_eq!(count_users(&f.pool, f.org).await, 1, "no user was created");
    assert_eq!(
        refusal(&f.pool, f.org, &f.club, "sub-club", json!({})).await,
        None
    );

    store_org_rules(&f.pool, f.org, json!({ "not": "a list" })).await;
    let failed = admit(&f.pool, f.org, &f.club, "sub-club", json!({})).await;
    assert_eq!(failed.expect_err("a fault").problem().status, 500);
}

/// Step 2 reads the rules in the sign-in's transaction, not from the provider row its caller
/// read earlier: a list stored after the caller read the provider applies.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_rules_are_read_at_sign_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    member(&f.pool, f.org, &f.sso, "sub-pat", json!({})).await;
    store_provider_rules(&f.pool, f.org, &f.sso, json!([usa()])).await;
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", json!({})).await,
        REFUSED
    );
}

/// Coordinator decision (the refusal marker): when the rules refuse a known user's provider
/// sign-in, the user is marked outside the rolled-back writes; a local sign-in, with or
/// without a local identity an enrollment link created, then refuses them; a provider sign-in
/// that passes the rules clears the marker and local sign-in works again. A new person the
/// rules refuse has no user to mark.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_provider_user_cannot_sign_in_locally_until_the_rules_pass(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    let pat = member(
        &f.pool,
        f.org,
        &f.sso,
        "sub-pat",
        json!({ "subdivision": "USA" }),
    )
    .await;
    let local = local_provider(&f.pool, f.org).await;
    let identity = NewIdentity {
        user_id: pat.id,
        provider_id: local.id,
        subject: pat.id.to_string(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("a local identity, as an enrollment link makes");
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    assert!(
        complete_local(&f.pool, f.org, pat.id).await.is_ok(),
        "no marker yet"
    );

    let moved = json!({ "subdivision": "CAN" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-pat", moved).await,
        REFUSED
    );
    let marker = stepclimb_core::admission_rules::refused_by(&f.pool, scope(f.org), pat.id)
        .await
        .expect("read the marker");
    assert_eq!(marker.as_deref(), Some("sso"));
    assert_eq!(
        complete_local(&f.pool, f.org, pat.id).await.err(),
        Some("auth.admission_refused")
    );
    let pat_subject = pat.id.to_string();
    assert_eq!(
        refusal(&f.pool, f.org, &local, &pat_subject, json!({})).await,
        Some((Step::Rules, "auth.admission_refused")),
        "admit at the local provider checks the marker too"
    );

    let new = json!({ "subdivision": "CAN" });
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-new", new).await,
        REFUSED
    );
    assert_eq!(
        count_users(&f.pool, f.org).await,
        1,
        "no user for the new person"
    );

    let back = json!({ "subdivision": "USA" });
    assert_eq!(refusal(&f.pool, f.org, &f.sso, "sub-pat", back).await, None);
    let marker = stepclimb_core::admission_rules::refused_by(&f.pool, scope(f.org), pat.id)
        .await
        .expect("read the marker");
    assert_eq!(marker, None);
    assert!(complete_local(&f.pool, f.org, pat.id).await.is_ok());
}

/// A local sign-in of `user` through `admission::complete_local`, committed: `Ok` with a
/// session, or the refusal's code.
async fn complete_local(pool: &PgPool, org: Uuid, user: Uuid) -> Result<(), &'static str> {
    let mut tx = pool.begin().await.expect("begin");
    let completed =
        admission::complete_local(&mut tx, scope(org), user, now(), &HeaderMap::new()).await;
    tx.commit().await.expect("commit");
    match completed {
        Ok(started) => {
            assert_eq!(started.session.user_id, user);
            Ok(())
        }
        Err(admission::Completion::Refused(refusal)) => Err(refusal.code()),
        Err(admission::Completion::Failed(error)) => panic!("a fault: {error:?}"),
    }
}

/// A local sign-in runs no rules: it carries no mapped attributes, so an organization-wide rule
/// would otherwise refuse every local account.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_sign_in_without_a_marker_runs_no_rules(pool: PgPool) {
    let f = Fixture::new(pool).await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let pat = user_in(&f.pool, f.org).await;
    assert_eq!(complete_local(&f.pool, f.org, pat.id).await, Ok(()));
}

/// The same through `admit`: a sign-in at the local provider of a local identity runs no
/// rules either.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn admit_runs_no_rules_for_the_local_provider(pool: PgPool) {
    let f = Fixture::new(pool).await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let local = local_provider(&f.pool, f.org).await;
    let pat = member(&f.pool, f.org, &local, "local-pat", json!({})).await;
    let admitted = admit(&f.pool, f.org, &local, "local-pat", json!({}))
        .await
        .expect("answers");
    assert!(matches!(admitted, Admission::Admitted(user) if user.id == pat.id));
}

/// B23, Q2 (a): `GET` returns the organization's list and every outside provider's, by key, an
/// empty list for one without rules and no entry for the local provider.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_returns_every_list(pool: PgPool) {
    let f = Fixture::new(pool).await;
    local_provider(&f.pool, f.org).await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let region = json!({ "attribute": "region", "op": "matches", "value": "e.*" });
    store_provider_rules(&f.pool, f.org, &f.club, json!([region])).await;
    let cookie = f.holder(&[AdmissionManage::KEY]).await;

    let read = f.send(Method::GET, &cookie, None).await;

    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_eq!(
        read.body,
        json!({
            "signup_allowed": false,
            "sync_only": false,
            "rules": {
                "organization": [usa()],
                "providers": { "club": [region], "sso": [] },
            },
        })
    );
    assert_schema("AdmissionSettings", &read.body);
}

/// B23, spec 09 B22: an admin replaces the organization's list and one provider's; each list
/// that changed gets one audit entry, the provider's other config members stay, the switches
/// are untouched, and admission applies what was written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admin_writes_lists_each_audited_and_admission_applies_them(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    let cookie = f.admin().await;
    let entries_before = audit::list(&f.pool, scope(f.org), 100).await.unwrap().len();
    let region = json!({ "attribute": "region", "op": "in", "value": ["east", "west"] });
    let rated = json!({ "any": [
        { "attribute": "rating", "op": "equals", "value": 5 },
        { "attribute": "rating", "op": "exists" }
    ] });

    let written = f
        .put(
            &cookie,
            json!({ "rules": { "organization": [rated], "providers": { "club": [region] } } }),
        )
        .await;

    assert_eq!(written.status, StatusCode::OK, "{written:?}");
    assert_eq!(
        written.body,
        json!({
            "signup_allowed": true,
            "sync_only": false,
            "rules": { "organization": [rated], "providers": { "club": [region], "sso": [] } },
        })
    );
    assert_schema("AdmissionSettings", &written.body);
    let club = identity_providers::get_by_key(&f.pool, scope(f.org), "club")
        .await
        .unwrap()
        .unwrap();
    let mut without_rules = club.config.clone();
    assert_eq!(
        without_rules.remove("admission_rules"),
        Some(json!([region]))
    );
    assert_eq!(
        without_rules, f.club.config,
        "the rest of the config is unchanged"
    );
    assert_eq!(club.updated_at, now());

    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    let new: Vec<_> = entries[entries_before..].iter().collect();
    assert_eq!(new.len(), 2, "{new:?}");
    let org_entry = new
        .iter()
        .find(|e| e.action == "organization.update_settings")
        .expect("an organization entry");
    assert_eq!(org_entry.target_id, f.org);
    assert_eq!(
        org_entry.data,
        json!({
            "before": { "admission": { "rules": [] } },
            "after": { "admission": { "rules": [rated] } },
        })
    );
    let provider_entry = new
        .iter()
        .find(|e| e.action == "identity_provider.update")
        .expect("a provider entry");
    assert_eq!(provider_entry.target_id, f.club.id);
    assert_eq!(
        provider_entry.data,
        json!({ "before": { "admission_rules": [] }, "after": { "admission_rules": [region] } })
    );
    assert!(matches!(provider_entry.actor, Actor::User { ip, .. } if ip == CLIENT.ip()));

    let club_ok = json!({ "region": "east", "rating": 1 });
    assert_eq!(refusal(&f.pool, f.org, &club, "sub-1", club_ok).await, None);
    let club_far = json!({ "region": "north", "rating": 1 });
    assert_eq!(
        refusal(&f.pool, f.org, &club, "sub-2", club_far).await,
        REFUSED
    );
    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-3", json!({})).await,
        REFUSED
    );
}

/// The partial `PUT` (I2, I4 notes): a body naming one provider's list leaves the
/// organization's list, the other providers' lists and the switches as stored; `[]` empties a
/// list and removes the member from the provider's config.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_put_replaces_only_the_lists_it_names(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup().await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let region = json!({ "attribute": "region", "op": "exists" });
    store_provider_rules(&f.pool, f.org, &f.club, json!([region])).await;
    let cookie = f.admin().await;
    let rated = json!({ "attribute": "rating", "op": "exists" });

    let one = f
        .put(
            &cookie,
            json!({ "rules": { "providers": { "sso": [rated] } } }),
        )
        .await;

    assert_eq!(one.status, StatusCode::OK, "{one:?}");
    assert_eq!(one.body["signup_allowed"], true);
    assert_eq!(
        one.body["rules"],
        json!({ "organization": [usa()], "providers": { "club": [region], "sso": [rated] } })
    );

    let emptied = f
        .put(
            &cookie,
            json!({ "rules": { "providers": { "club": [] }, "organization": [] } }),
        )
        .await;
    assert_eq!(emptied.status, StatusCode::OK, "{emptied:?}");
    assert_eq!(
        emptied.body["rules"],
        json!({ "organization": [], "providers": { "club": [], "sso": [rated] } })
    );
    let stored: (Value, Value) = sqlx::query_as(
        "SELECT o.settings -> 'admission', p.config
         FROM organizations o JOIN identity_providers p ON p.org_id = o.id
         WHERE o.id = $1 AND p.key = 'club'",
    )
    .bind(f.org)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(stored.0, json!({ "signup_allowed": true }));
    assert!(stored.1.get("admission_rules").is_none(), "{}", stored.1);
}

/// Spec 09 B22: a `PUT` whose lists equal the stored ones writes nothing. A body that names no
/// list needs no more than `admission.manage`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_put_that_changes_no_list_writes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    store_org_rules(&f.pool, f.org, json!([usa()])).await;
    let manager = f.holder(&[AdmissionManage::KEY]).await;
    let cookie = f.admin().await;
    let before = f.snapshot().await;
    for body in [
        json!({ "rules": {} }),
        json!({ "rules": { "providers": {} } }),
    ] {
        let unchanged = f.put(&manager, body.clone()).await;
        assert_eq!(unchanged.status, StatusCode::OK, "{body}: {unchanged:?}");
        assert_eq!(f.snapshot().await, before, "{body}");
    }

    for body in [
        json!({ "rules": {} }),
        json!({ "rules": { "organization": [usa()] } }),
        json!({ "rules": { "providers": { "sso": [], "club": [] } } }),
    ] {
        let unchanged = f.put(&cookie, body.clone()).await;
        assert_eq!(unchanged.status, StatusCode::OK, "{body}: {unchanged:?}");
        assert_eq!(
            unchanged.body["rules"]["organization"],
            json!([usa()]),
            "{body}"
        );
        assert_eq!(f.snapshot().await, before, "{body}");
    }
}

/// Coordinator decision (escalation): rules can refuse any user who signs in through an outside
/// provider, so a holder of `admission.manage` without every catalog permission cannot name a
/// list, even one equal to the stored list or one that would not validate: the check runs
/// before the lists are read, so the answer says nothing about the attributes providers map.
/// The refusal writes nothing, a switch in the same body included.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn changing_a_list_needs_every_permission(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let cookie = f.holder(&[AdmissionManage::KEY]).await;
    let before = f.snapshot().await;

    for body in [
        json!({ "rules": { "organization": [usa()] } }),
        json!({ "rules": { "organization": [] } }),
        json!({ "rules": { "organization": [{ "attribute": "secret_name", "op": "exists" }] } }),
        json!({ "rules": { "providers": { "nope": [] } } }),
        json!({ "signup_allowed": true, "rules": { "providers": { "sso": [usa()] } } }),
    ] {
        let refused = f.put(&cookie, body.clone()).await;
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::FORBIDDEN, "permission.escalation"),
            "{body}"
        );
        assert_eq!(f.snapshot().await, before, "{body}");
    }
    let switch_only = f.put(&cookie, json!({ "signup_allowed": true })).await;
    assert_eq!(switch_only.status, StatusCode::OK, "{switch_only:?}");
}

/// Validation on save: each problem is a 422 entry with a pointer to the member, and nothing is
/// written. An attribute the list's providers do not map is `admission.attribute_unknown`
/// (the organization's list may name any provider's), a key that is not an outside provider
/// `admission.provider_unknown`, and the list is capped at 20 entries.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_invalid_change_names_each_problem_and_writes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let cookie = f.admin().await;
    let before = f.snapshot().await;

    let refused = f
        .put(
            &cookie,
            json!({ "rules": {
                "organization": [
                    { "attribute": "region", "op": "exists" },
                    { "attribute": "rank", "op": "exists" },
                    { "attribute": "rating", "op": "like", "value": "S" }
                ],
                "providers": {
                    "club": [{ "attribute": "subdivision", "op": "exists" }],
                    "local": [],
                    "nope": [],
                    "sso": [{ "any": [usa(), { "attribute": "rating", "op": "in", "value": [] }] }]
                }
            } }),
        )
        .await;

    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::UNPROCESSABLE_ENTITY, "request.invalid"),
        "{refused:?}"
    );
    let expected = [
        (
            "/rules/organization/1/attribute",
            "admission.attribute_unknown",
        ),
        ("/rules/organization/2/op", "admission.rule_invalid"),
        (
            "/rules/providers/club/0/attribute",
            "admission.attribute_unknown",
        ),
        ("/rules/providers/local", "admission.provider_unknown"),
        ("/rules/providers/nope", "admission.provider_unknown"),
        (
            "/rules/providers/sso/0/any/1/value",
            "admission.rule_invalid",
        ),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(f, c)| ((*f).to_owned(), (*c).to_owned()))
        .collect();
    assert_eq!(refused.errors(), expected);
    assert_eq!(f.snapshot().await, before);

    let many: Vec<Value> = (0..30).map(|_| json!({ "op": "exists" })).collect();
    let capped = f
        .put(&cookie, json!({ "rules": { "organization": many } }))
        .await;
    assert_eq!(capped.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(capped.errors().len(), 20);

    for body in [
        json!({ "rules": null }),
        json!({ "rules": { "organization": {} } }),
        json!({ "rules": { "providers": { "sso": null } } }),
        json!({ "rules": { "webhook": [] } }),
    ] {
        let malformed = f.put(&cookie, body.clone()).await;
        assert_eq!(
            (malformed.status, malformed.code()),
            (StatusCode::BAD_REQUEST, "request.malformed"),
            "{body}"
        );
    }
    assert_eq!(f.snapshot().await, before);
}

/// A stored list that does not parse makes `GET` a 500 and blocks a `PUT` that leaves it as it
/// is, writing nothing; a `PUT` that names it replaces it, with `before: null` in its audit
/// entry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_list_that_does_not_parse_can_be_replaced(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let newer = json!([{ "attribute": "region", "op": "starts_with", "value": "e" }]);
    store_provider_rules(&f.pool, f.org, &f.club, newer).await;
    let cookie = f.admin().await;
    let before = f.snapshot().await;

    let read = f.send(Method::GET, &cookie, None).await;
    assert_eq!(read.status, StatusCode::INTERNAL_SERVER_ERROR, "{read:?}");
    let elsewhere = f
        .put(
            &cookie,
            json!({ "signup_allowed": true, "rules": { "organization": [usa()] } }),
        )
        .await;
    assert_eq!(
        elsewhere.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{elsewhere:?}"
    );
    assert_eq!(f.snapshot().await, before);

    let region = json!({ "attribute": "region", "op": "exists" });
    let replaced = f
        .put(
            &cookie,
            json!({ "rules": { "providers": { "club": [region] } } }),
        )
        .await;
    assert_eq!(replaced.status, StatusCode::OK, "{replaced:?}");
    assert_eq!(replaced.body["rules"]["providers"]["club"], json!([region]));
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    let entry = entries.last().expect("an entry");
    assert_eq!(entry.action, "identity_provider.update");
    assert_eq!(
        entry.data,
        json!({ "before": { "admission_rules": null }, "after": { "admission_rules": [region] } })
    );
    let read = f.send(Method::GET, &cookie, None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
}

/// T20 for the rules: organization A's admin writes A's lists; organization B, with a provider
/// of the same key, keeps its own (empty) lists, its sign-ins run none of A's rules, and A's
/// `PUT` cannot name B's provider.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn one_organizations_rules_never_reach_another(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let org_b = org(&f.pool, "org-b").await;
    let sso_b = oidc_provider(&f.pool, org_b, "sso").await;
    let only_b = oauth2_provider(&f.pool, org_b, "only-b").await;
    member(&f.pool, org_b, &sso_b, "sub-b", json!({})).await;
    member(&f.pool, f.org, &f.sso, "sub-a", json!({})).await;
    let b_before: Value =
        sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE o.id = $1")
            .bind(org_b)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let cookie = f.admin().await;

    let written = f
        .put(
            &cookie,
            json!({ "rules": { "organization": [usa()], "providers": { "sso": [usa()] } } }),
        )
        .await;
    assert_eq!(written.status, StatusCode::OK, "{written:?}");
    let foreign = f
        .put(
            &cookie,
            json!({ "rules": { "providers": { "only-b": [] } } }),
        )
        .await;
    assert_eq!(
        foreign.errors(),
        [(
            "/rules/providers/only-b".to_owned(),
            "admission.provider_unknown".to_owned()
        )]
    );

    assert_eq!(
        refusal(&f.pool, f.org, &f.sso, "sub-a", json!({})).await,
        REFUSED
    );
    assert_eq!(
        refusal(&f.pool, org_b, &sso_b, "sub-b", json!({})).await,
        None
    );
    let b_after: Value =
        sqlx::query_scalar("SELECT to_jsonb(o) FROM organizations o WHERE o.id = $1")
            .bind(org_b)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(b_after, b_before);
    for provider in [&sso_b, &only_b] {
        let stored = identity_providers::get_by_key(&f.pool, scope(org_b), &provider.key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&stored, provider, "B's provider is unchanged");
    }
}
