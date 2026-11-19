// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /api/v1/me` over the whole router (docs/specs/m0/03-users-and-identities.md B21, B22,
//! T6, T17; its `features`, docs/specs/m0/10-entitlements-framework.md B12, T9). The 401, 2xx and
//! cross-organization cases of the permission suite are in `tests/permissions.rs` (its `getMe`
//! entry). A test plan is written by SQL and the organization pointed at it, since M0 has no API
//! that changes a plan.
//!
//! One organization, so the production request-organization resolver serves it.
//! `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    clock::ManualClock,
    feature,
    id::{Uuid, new_id},
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles,
    users::{self, NewUser, Status},
};
use stepclimb_server::{AppState, openapi, router, session, web_origin::WebOrigin};
use tower::ServiceExt;

mod common;

const ME: &str = "/api/v1/me";

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document as JSON")
}

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    app: Router,
    org: Uuid,
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

    fn cache_control(&self) -> Option<&str> {
        self.headers
            .get(header::CACHE_CONTROL)
            .map(|v| v.to_str().expect("ASCII"))
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = organizations::create(&pool, "example", "Example Org", now())
            .await
            .expect("create organization")
            .id;
        let clock = ManualClock::new(now());
        let app = router(AppState {
            db: pool.clone(),
            clock: clock.clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage: None,
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        });
        Self {
            pool,
            clock,
            app,
            org,
        }
    }

    /// Sam Ortiz, made at sign-in through an identity provider, in New York.
    async fn sam(&self) -> Uuid {
        let new_user = NewUser {
            display_name: "Sam Ortiz".into(),
            given_name: "Sam".into(),
            family_name: "Ortiz".into(),
            email: Some("sam@example.test".into()),
            locale: None,
            tz: "America/New_York".into(),
            source: Source::Idp,
            external_id: Some("E1".into()),
        };
        users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id
    }

    /// What a sign-in does: a new session for `user`; the `Cookie` value.
    async fn sign_in(&self, user: Uuid) -> String {
        let started = session::start(
            &self.pool,
            scope(self.org),
            user,
            &HeaderMap::new(),
            self.clock.clock().now(),
        )
        .await
        .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    async fn set_status(&self, user: Uuid, status: Status) {
        users::set_status(
            &self.pool,
            scope(self.org),
            user,
            status,
            now(),
            Actor::Instance,
        )
        .await
        .expect("set status");
    }

    async fn get_me(&self, headers: &[(header::HeaderName, &str)]) -> Answer {
        let mut request = Request::get(ME).header("sec-fetch-site", "same-origin");
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::empty()).expect("a valid request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        let body = serde_json::from_slice(&bytes).expect("a JSON body");
        Answer {
            status,
            headers,
            body,
        }
    }

    async fn me_with_cookie(&self, cookie: &str) -> Answer {
        self.get_me(&[(header::COOKIE, cookie)]).await
    }
}

/// Every catalog feature key, sorted: what the `unlimited` plan lists.
fn every_feature() -> Vec<&'static str> {
    let mut keys = feature::ALL.to_vec();
    keys.sort_unstable();
    keys
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_a_session_user_gets_the_b22_shape(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let cookie = fixture.sign_in(sam).await;
    let answer = fixture.me_with_cookie(&cookie).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body,
        json!({
            "user": {
                "id": sam.to_string(),
                "display_name": "Sam Ortiz",
                "given_name": "Sam",
                "family_name": "Ortiz",
                "email": "sam@example.test",
                "locale": "en",
                "tz": "America/New_York",
                "status": "active",
                "source": "idp",
            },
            "organization": {
                "id": fixture.org.to_string(),
                "slug": "example",
                "name": "Example Org",
            },
            "grants": [],
            "features": every_feature(),
            "appearance": { "theme": null, "density": "comfortable" },
            "default_appearance": "system",
            "embedding": { "return_origins": [] },
        })
    );
    let errors = common::schema_errors(&document(), "Me", &answer.body);
    assert!(errors.is_empty(), "{errors:#?}");
}

/// Spec 08 B43: the user's personal data is never kept by a shared or browser cache, and
/// neither is a refusal.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_answer_and_its_refusals_are_no_store(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let cookie = fixture.sign_in(sam).await;
    let answer = fixture.me_with_cookie(&cookie).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.cache_control(), Some("no-store"));
    let anonymous = fixture.get_me(&[]).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.cache_control(), Some("no-store"));
    fixture.set_status(sam, Status::Suspended).await;
    let suspended = fixture.me_with_cookie(&cookie).await;
    assert_eq!(suspended.status, StatusCode::UNAUTHORIZED);
    assert_eq!(suspended.cache_control(), Some("no-store"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_without_an_email_gets_null(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
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
    let pat = users::create(&fixture.pool, scope(fixture.org), &new_user, now())
        .await
        .expect("create user")
        .id;
    let answer = fixture.me_with_cookie(&fixture.sign_in(pat).await).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["user"]["email"], Value::Null);
    assert_eq!(answer.body["user"]["source"], "local");
    let errors = common::schema_errors(&document(), "Me", &answer.body);
    assert!(errors.is_empty(), "{errors:#?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_anonymous_and_unknown_sessions_answer_401(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let anonymous = fixture.get_me(&[]).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.code(), "auth.unauthenticated");
    let unknown = fixture
        .me_with_cookie("__Host-sc_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
        .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.code(), "auth.unauthenticated");
}

/// A cookie beside a bearer header never authenticates the request (spec 08 B16): a malformed
/// key answers its own 401. The owned-key case of T17 is in `tests/service_accounts.rs`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_bearer_request_is_not_answered_for_the_cookie_beside_it(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cookie = fixture.sign_in(fixture.sam().await).await;
    let answer = fixture
        .get_me(&[
            (header::COOKIE, cookie.as_str()),
            (header::AUTHORIZATION, "Bearer sc_unknown"),
        ])
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{answer:?}");
    assert_eq!(answer.code(), "auth.api_key_invalid");
    assert!(answer.body.get("user").is_none());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_suspension_ends_the_session_on_its_next_request_and_unsuspending_restores_sign_in(
    pool: PgPool,
) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let cookie = fixture.sign_in(sam).await;
    assert_eq!(fixture.me_with_cookie(&cookie).await.status, StatusCode::OK);

    fixture.set_status(sam, Status::Suspended).await;
    let suspended = fixture.me_with_cookie(&cookie).await;
    assert_eq!(suspended.status, StatusCode::UNAUTHORIZED, "{suspended:?}");
    assert_eq!(suspended.code(), "auth.suspended");
    assert!(suspended.body.get("user").is_none());

    fixture.set_status(sam, Status::Active).await;
    let new_cookie = fixture.sign_in(sam).await;
    let answer = fixture.me_with_cookie(&new_cookie).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["user"]["id"], sam.to_string());
    assert_eq!(answer.body["user"]["status"], "active");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_session_answers_its_own_user(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let other = NewUser {
        display_name: "Robin Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let robin = users::create(&fixture.pool, scope(fixture.org), &other, now())
        .await
        .expect("create user")
        .id;
    let sam_cookie = fixture.sign_in(sam).await;
    let robin_cookie = fixture.sign_in(robin).await;
    let for_sam = fixture.me_with_cookie(&sam_cookie).await;
    let for_robin = fixture.me_with_cookie(&robin_cookie).await;
    assert_eq!(for_sam.body["user"]["id"], sam.to_string());
    assert_eq!(for_robin.body["user"]["id"], robin.to_string());
    assert_eq!(for_robin.body["user"]["display_name"], "Robin Example");
}

/// Assign the built-in role `key` to `user` at `scope`.
async fn assign_builtin(fixture: &Fixture, user: Uuid, key: &str, at: Scope) {
    let role = sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
        .bind(key)
        .fetch_one(&fixture.pool)
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
        &fixture.pool,
        scope(fixture.org),
        &assignment,
        now(),
        Actor::Instance,
    )
    .await
    .expect("create assignment");
}

/// Spec 09 T17: an account admin of Tower has one grant, at Tower, with B5's list sorted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_an_account_admin_of_tower_gets_one_sorted_grant(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.expect("sync roles");
    let fixture = Fixture::new(pool).await;
    let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(fixture.org)
        .fetch_one(&fixture.pool)
        .await
        .expect("the root");
    let tower = accounts::create(
        &fixture.pool,
        scope(fixture.org),
        root,
        "Tower",
        now(),
        Actor::Instance,
    )
    .await
    .expect("create Tower")
    .id;
    let sam = fixture.sam().await;
    assign_builtin(&fixture, sam, "account_admin", Scope::Account(tower)).await;
    // Organization admin at Tower adds only its account-grantable permissions, which are
    // Account admin's: organization-only ones take no effect there, so no organization grant.
    assign_builtin(&fixture, sam, "org_admin", Scope::Account(tower)).await;
    // A role with no permissions adds no grant.
    assign_builtin(&fixture, sam, "grader", Scope::Organization).await;

    let answer = fixture.me_with_cookie(&fixture.sign_in(sam).await).await;

    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body["grants"],
        json!([{
            "scope": { "type": "account", "id": tower.to_string() },
            "permissions": [
                "account.manage", "account.view", "course.content_edit", "course.create",
                "course.manage", "course.view", "course.view_all", "enrollment.manage",
                "enrollment.view", "progress.view", "role.assign", "role.view", "user.view"
            ],
        }])
    );
    let errors = common::schema_errors(&document(), "Me", &answer.body);
    assert!(errors.is_empty(), "{errors:#?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_grant_comes_first_and_a_removed_assignment_is_gone(pool: PgPool) {
    roles::sync_builtin(&pool, now()).await.expect("sync roles");
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    assign_builtin(&fixture, sam, "org_admin", Scope::Organization).await;
    let cookie = fixture.sign_in(sam).await;

    let admin = fixture.me_with_cookie(&cookie).await;
    sqlx::query("DELETE FROM role_assignments WHERE user_id = $1")
        .bind(sam)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let removed = fixture.me_with_cookie(&cookie).await;

    let grants = admin.body["grants"].as_array().expect("grants");
    assert_eq!(grants.len(), 1, "{grants:?}");
    assert_eq!(
        grants[0]["scope"],
        json!({ "type": "organization", "id": fixture.org.to_string() })
    );
    // `course.participate` is grantable only at course scope: not effective at the organization.
    let mut every: Vec<&str> = stepclimb_core::perm::ALL
        .iter()
        .filter(|p| !p.course_only())
        .map(|p| p.key())
        .collect();
    every.sort_unstable();
    assert_eq!(grants[0]["permissions"], json!(every));
    assert_eq!(removed.body["grants"], json!([]));
}

// Spec 10 T9: the features `/me` lists.

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_unlimited_lists_every_catalog_feature_sorted(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let answer = fixture.me_with_cookie(&fixture.sign_in(sam).await).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(answer.body["features"], json!(every_feature()));
    assert_eq!(every_feature().len(), feature::ALL.len());
}

/// A test plan lists only the catalog features it stores as `true` (B7); a key the catalog does
/// not have is never listed. A plan change applies on the next request (B8).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_test_plan_lists_only_the_features_it_enables(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let cookie = fixture.sign_in(sam).await;
    let plan = new_id();
    sqlx::query("INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3, '{}'::jsonb)")
        .bind(plan)
        .bind(format!("test_{}", plan.simple()))
        .bind(json!({
            "oidc_providers": true,
            "api_keys": true,
            "custom_roles": false,
            "custom_branding": "true",
            "retired_feature": true,
        }))
        .execute(&fixture.pool)
        .await
        .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(fixture.org)
        .bind(plan)
        .execute(&fixture.pool)
        .await
        .expect("move to the plan");

    let answer = fixture.me_with_cookie(&cookie).await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert_eq!(
        answer.body["features"],
        json!(["api_keys", "oidc_providers"])
    );
    let errors = common::schema_errors(&document(), "Me", &answer.body);
    assert!(errors.is_empty(), "{errors:#?}");

    sqlx::query("UPDATE plans SET features = '{}'::jsonb WHERE id = $1")
        .bind(plan)
        .execute(&fixture.pool)
        .await
        .expect("disable every feature");
    let answer = fixture.me_with_cookie(&cookie).await;
    assert_eq!(answer.body["features"], json!([]));
}

/// An organization without its `org_plans` row fails closed: no features are guessed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_without_a_plan_row_answers_500(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let sam = fixture.sam().await;
    let cookie = fixture.sign_in(sam).await;
    sqlx::query("DELETE FROM org_plans WHERE org_id = $1")
        .bind(fixture.org)
        .execute(&fixture.pool)
        .await
        .expect("delete the plan row");
    let answer = fixture.me_with_cookie(&cookie).await;
    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{answer:?}"
    );
    assert_eq!(answer.code(), "internal.error");
}
