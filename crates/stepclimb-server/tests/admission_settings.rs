// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission switches over the router (docs/specs/m0/07-admission.md B12, B23, Q2; T19 and
//! T20 for `/api/v1/admin/admission`). The permission suite's four cases and cross-organization
//! case for both operations are in `tests/permissions.rs`; these tests cover the values, the
//! audit entry, a change that changes nothing, a partial body that keeps the other switch,
//! malformed bodies, and that admission reads what the route writes (both switches), in its own
//! organization only.
//!
//! Requests name their organization with the `test-support` header of `request_org`, since one
//! test has two organizations. `session::start` stands in for the sign-in specs.
//! All data is synthetic. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::LazyLock;
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
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    admission_settings::{self, AdmissionPatch},
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identity_providers::{self, IdentityProvider},
    organizations,
    perm::{AdmissionManage, Permission},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::providers::Providers;
use stepclimb_server::{
    AppState,
    admission::{self, Admission, Context, Profile, Verified},
    audit::ClientAddress,
    request_org::TEST_ORG_HEADER,
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1); audit entries record it.
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const PATH: &str = "/api/v1/admin/admission";
const SETTINGS: &str = "/api/v1/organization/settings";

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

fn assert_schema(name: &str, value: &Value) {
    let document =
        serde_json::to_value(stepclimb_server::openapi::document()).expect("the document");
    let errors = common::schema_errors(&document, name, value);
    assert!(errors.is_empty(), "{name}: {errors:#?}");
}

fn switches(signup_allowed: bool, sync_only: bool) -> Value {
    json!({ "signup_allowed": signup_allowed, "sync_only": sync_only })
}

/// The route's answer with these switches and no rules (`tests/admission_rules.rs` covers
/// the rules).
fn answer(signup_allowed: bool, sync_only: bool) -> Value {
    let mut answer = switches(signup_allowed, sync_only);
    answer["rules"] = json!({ "organization": [], "providers": {} });
    answer
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
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let state = AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        };
        let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app, org }
    }

    /// A signed-in user holding a custom role with `keys` at organization scope: the `Cookie`
    /// value and the user's id.
    async fn holder(&self, keys: &[&str]) -> (String, Uuid) {
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
        let user = users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id;
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
        (cookie.split(';').next().expect("a pair").to_owned(), user)
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
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
        self.send(Method::PUT, PATH, cookie, Some(body)).await
    }

    /// Store `sync_only` by SQL, beside whatever the route wrote.
    async fn store_sync_only(&self) {
        sqlx::query(
            r#"UPDATE organizations
               SET settings = settings || '{"admission": {"sync_only": true}}'::jsonb
               WHERE id = $1"#,
        )
        .bind(self.org)
        .execute(&self.pool)
        .await
        .expect("store sync_only");
    }

    /// The stored settings, `updated_at` and the number of audit entries.
    async fn snapshot(&self) -> (String, String, i64) {
        sqlx::query_as(
            "SELECT o.settings::text, o.updated_at::text,
                    (SELECT count(*) FROM audit_events a WHERE a.org_id = o.id)
             FROM organizations o WHERE o.id = $1",
        )
        .bind(self.org)
        .fetch_one(&self.pool)
        .await
        .expect("read the organization")
    }
}

/// An OpenID Connect provider of `org`, inserted by SQL: the provider routes are spec 06's.
async fn provider_in(pool: &PgPool, org: Uuid) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, 'sso', 'oidc', '{}'::jsonb)",
    )
    .bind(new_id())
    .bind(org)
    .execute(pool)
    .await
    .expect("insert provider");
    identity_providers::get_by_key(pool, scope(org), "sso")
        .await
        .expect("read provider")
        .expect("the provider exists")
}

/// The refusal code admission answers a person `org` has no user for, through `provider`, as a
/// provider's callback asks it; `None` when it admits them.
async fn new_person_refused(
    pool: &PgPool,
    org: Uuid,
    provider: &IdentityProvider,
) -> Option<&'static str> {
    let sign_in = Verified {
        provider,
        subject: "sub-new",
        profile: Profile::default(),
        email_verified: false,
        attributes: Map::new(),
    };
    let context = Context {
        scope: scope(org),
        client: ClientAddress(CLIENT.ip()),
        now: now(),
        keys: &KEY_RING,
        providers: &PROVIDERS,
    };
    let mut tx = pool.begin().await.expect("begin");
    let admission = admission::admit(&mut tx, &sign_in, &context)
        .await
        .expect("admission answers");
    tx.rollback().await.expect("roll back");
    match admission {
        Admission::Admitted(_) => None,
        Admission::Refused(refusal) => Some(refusal.code()),
    }
}

/// B12, B20: a new organization has both switches off.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_answers_both_switches_off_for_a_new_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;

    let read = f.send(Method::GET, PATH, &cookie, None).await;

    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_eq!(read.body, answer(false, false));
    assert_schema("AdmissionSettings", &read.body);
}

/// B23, spec 09 B22: `admission.manage` alone opens sign-up (no escalation rule); the change
/// is stored, shown by both reads, and audited once with the switches before and after, the
/// caller and the client address. Closing it again is a second change with its own entry.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn admission_manage_alone_changes_the_switches_and_each_change_is_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, user) = f.holder(&[AdmissionManage::KEY]).await;
    let (_, _, entries_before) = f.snapshot().await;

    let opened = f.put(&cookie, json!({ "signup_allowed": true })).await;

    assert_eq!(opened.status, StatusCode::OK, "{opened:?}");
    assert_eq!(opened.body, answer(true, false));
    assert_schema("AdmissionSettings", &opened.body);
    let read = f.send(Method::GET, PATH, &cookie, None).await;
    assert_eq!(read.body, answer(true, false));
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    assert_eq!(entries.len() as i64, entries_before + 1);
    let entry = entries.last().expect("an entry");
    assert_eq!(entry.action.as_str(), "organization.update_settings");
    assert_eq!(entry.target_id, f.org);
    assert!(
        matches!(entry.actor, Actor::User { id, ip } if id == user && ip == CLIENT.ip()),
        "{:?}",
        entry.actor
    );
    assert_eq!(
        entry.data,
        json!({
            "before": { "admission": switches(false, false) },
            "after": { "admission": switches(true, false) },
        })
    );

    let closed = f.put(&cookie, json!({ "signup_allowed": false })).await;
    assert_eq!(closed.body, answer(false, false));
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    assert_eq!(entries.len() as i64, entries_before + 2);
}

/// The organization settings show the switches the route wrote.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_organization_settings_show_the_written_switches(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let all: Vec<&str> = stepclimb_core::perm::ALL.iter().map(|p| p.key()).collect();
    let (admin, _) = f.holder(&all).await;

    f.put(&admin, json!({ "signup_allowed": true })).await;

    let read = f.send(Method::GET, SETTINGS, &admin, None).await;
    assert_eq!(read.status, StatusCode::OK, "{read:?}");
    assert_eq!(read.body["admission"], switches(true, false));
    assert_schema("OrgSettings", &read.body);
}

/// Spec 09 B22: a `PUT` that changes nothing writes nothing: no settings change, no
/// `updated_at` bump, no audit entry. That is `{}`, and any body naming only current values.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_put_that_changes_nothing_writes_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;
    for signup in [false, true] {
        f.put(&cookie, json!({ "signup_allowed": signup })).await;
        let before = f.snapshot().await;

        for body in [
            json!({}),
            json!({ "signup_allowed": signup }),
            json!({ "sync_only": false }),
            switches(signup, false),
        ] {
            let unchanged = f.put(&cookie, body.clone()).await;
            assert_eq!(unchanged.status, StatusCode::OK, "{body}: {unchanged:?}");
            assert_eq!(unchanged.body, answer(signup, false), "{body}");
            assert_eq!(f.snapshot().await, before, "{body}");
        }
    }
}

/// Coordinator decision (lost update): a `PUT` changes only the switches it names, so it never
/// writes back a stale value of a switch another admin changed. Admin X's
/// `{signup_allowed: true}` keeps the `sync_only` stored before it, and admin Y's
/// `{sync_only: false}` keeps the sign-up X opened.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_put_keeps_the_switch_it_does_not_name(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (x, _) = f.holder(&[AdmissionManage::KEY]).await;
    let (y, _) = f.holder(&[AdmissionManage::KEY]).await;
    f.store_sync_only().await;

    let opened = f.put(&x, json!({ "signup_allowed": true })).await;

    assert_eq!(opened.status, StatusCode::OK, "{opened:?}");
    assert_eq!(opened.body, answer(true, true));
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    assert_eq!(
        entries.last().expect("an entry").data["after"]["admission"],
        switches(true, true)
    );

    let closed = f.put(&y, json!({ "sync_only": false })).await;

    assert_eq!(closed.status, StatusCode::OK, "{closed:?}");
    assert_eq!(closed.body, answer(true, false));
    let read = f.send(Method::GET, PATH, &x, None).await;
    assert_eq!(read.body, answer(true, false));
}

/// Every field is validated: a body with an unknown member, a `rules` that is not an object, a
/// `null` or a string switch is malformed and writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_malformed_body_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;
    let before = f.snapshot().await;

    for body in [
        json!({ "signup_allowed": true, "rules": [] }),
        json!({ "signup_allowed": true, "webhook": {} }),
        json!({ "signup_allowed": null }),
        json!({ "sync_only": null }),
        json!({ "signup_allowed": "true" }),
        json!({ "sync_only": 0 }),
    ] {
        let refused = f.put(&cookie, body.clone()).await;
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::BAD_REQUEST, "request.malformed"),
            "{body}"
        );
    }
    assert_eq!(f.snapshot().await, before);
}

/// A member of the stored `admission` object this release does not know (one a newer release
/// wrote) survives a change of the switches.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_keeps_stored_admission_members_this_release_does_not_know(pool: PgPool) {
    let f = Fixture::new(pool).await;
    sqlx::query(
        r#"UPDATE organizations
           SET settings = settings || '{"admission": {"newer": [1]}}'::jsonb
           WHERE id = $1"#,
    )
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("store a newer member");
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;

    let opened = f.put(&cookie, json!({ "signup_allowed": true })).await;

    assert_eq!(opened.status, StatusCode::OK, "{opened:?}");
    let stored: Value = sqlx::query_scalar("SELECT settings -> 'admission' FROM organizations")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        stored,
        json!({ "newer": [1], "signup_allowed": true, "sync_only": false })
    );
}

/// B20 over the chain, and T20 for the switches: what the route writes is what admission reads,
/// and only in the caller's organization. Organization B exists before A's admin opens sign-up
/// in A; afterwards B's sign-up is still closed and its new people are refused, while A's are
/// admitted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn admission_applies_the_written_switch_in_its_own_organization_only(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let provider_a = provider_in(&f.pool, f.org).await;
    let org_b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization B")
        .id;
    let provider_b = provider_in(&f.pool, org_b).await;
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;
    let closed = Some("auth.signup_closed");
    assert_eq!(
        new_person_refused(&f.pool, f.org, &provider_a).await,
        closed
    );
    assert_eq!(
        new_person_refused(&f.pool, org_b, &provider_b).await,
        closed
    );

    let opened = f.put(&cookie, json!({ "signup_allowed": true })).await;
    assert_eq!(opened.status, StatusCode::OK, "{opened:?}");

    assert_eq!(new_person_refused(&f.pool, f.org, &provider_a).await, None);
    assert_eq!(
        new_person_refused(&f.pool, org_b, &provider_b).await,
        closed
    );
    let b = organizations::settings(&f.pool, scope(org_b))
        .await
        .unwrap();
    assert!(!b.admission.signup_allowed);
}

/// B11, B12 and T20 for `sync_only`: `admission.manage` turns it on with one audit entry, and
/// admission then refuses a new person `auth.not_provisioned` though sign-up is allowed, in the
/// caller's organization only; organization B, with sign-up allowed too, still admits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn sync_only_written_by_the_route_refuses_new_people_in_its_organization(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let provider_a = provider_in(&f.pool, f.org).await;
    let org_b = organizations::create(&f.pool, "org-b", "Org B", now())
        .await
        .expect("create organization B")
        .id;
    let provider_b = provider_in(&f.pool, org_b).await;
    let (cookie, _) = f.holder(&[AdmissionManage::KEY]).await;
    let opened = f.put(&cookie, json!({ "signup_allowed": true })).await;
    assert_eq!(opened.status, StatusCode::OK, "{opened:?}");
    let open_b = AdmissionPatch {
        signup_allowed: Some(true),
        ..AdmissionPatch::default()
    };
    admission_settings::update(&f.pool, scope(org_b), open_b, now(), Actor::Instance)
        .await
        .expect("open sign-up in B");
    let before = audit::list(&f.pool, scope(f.org), 100).await.unwrap().len();

    let on = f.put(&cookie, json!({ "sync_only": true })).await;

    assert_eq!(on.status, StatusCode::OK, "{on:?}");
    assert_eq!(
        (&on.body["signup_allowed"], &on.body["sync_only"]),
        (&json!(true), &json!(true))
    );
    let entries = audit::list(&f.pool, scope(f.org), 100).await.unwrap();
    assert_eq!(entries.len(), before + 1);
    assert_eq!(
        entries.last().expect("an entry").data["after"]["admission"],
        switches(true, true)
    );
    let refused = new_person_refused(&f.pool, f.org, &provider_a).await;
    assert_eq!(refused, Some("auth.not_provisioned"));
    assert_eq!(new_person_refused(&f.pool, org_b, &provider_b).await, None);

    let off = f.put(&cookie, json!({ "sync_only": false })).await;
    assert_eq!(off.status, StatusCode::OK, "{off:?}");
    assert_eq!(new_person_refused(&f.pool, f.org, &provider_a).await, None);
}
