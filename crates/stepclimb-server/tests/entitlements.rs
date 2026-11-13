// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Require<Feature>`, the per-request entitlements and the limit check over HTTP
//! (docs/specs/m0/10-entitlements-framework.md B8 to B11; T5, T6, T7). Test-only routes take each
//! extractor; `session::start` stands in for the sign-in specs. A test plan is written by SQL and
//! an organization pointed at it, since M0 has no API that changes a plan. Two organizations
//! share the database, each request naming its own with the `test-support` organization header.
//! Needs `DATABASE_URL` (`just up`).

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, header},
    routing::{get, post},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    clock::ManualClock,
    feature::{self, Feature, Limit, TestSeal},
    id::{Uuid, new_id},
    organizations,
    perm::{AccountView, OrgManage},
    requirement::{FeatureKind, Requirement},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    entitlements::Entitlements,
    error::{ApiError, Problem},
    request_org::TEST_ORG_HEADER,
    require::{AccountTarget, Require},
    session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

/// A feature defined only in this test crate, so not in `feature::ALL`.
struct Gate;

impl TestSeal for Gate {}

impl Feature for Gate {
    const KEY: &'static str = "test_gate";
}

impl Requirement for Gate {
    type Kind = FeatureKind;
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

async fn gated(_: Require<Gate>) -> &'static str {
    "gated"
}

async fn catalog_gated(_: Require<feature::CustomRoles>) -> &'static str {
    "catalog"
}

async fn gated_account(_: Require<Gate>, require: Require<AccountView, AccountTarget>) -> String {
    require.target().id.to_string()
}

async fn two_features(
    _: Require<Gate>,
    _: Require<feature::CustomRoles>,
    _: Require<OrgManage>,
) -> &'static str {
    "both"
}

async fn wrong_order(_: Require<AccountView, AccountTarget>, _: Require<Gate>) -> &'static str {
    "unreachable"
}

async fn create_user(
    require: Require<OrgManage>,
    entitlements: Entitlements,
    State(state): State<AppState>,
) -> Result<&'static str, ApiError> {
    let scope = require.principal().scope();
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    entitlements.check_room(&mut tx, Limit::Users, 1).await?;
    users::create(&mut *tx, scope, &new_user("New User"), now())
        .await
        .map_err(|e| ApiError::internal(&e))?;
    tx.commit().await.map_err(|e| ApiError::internal(&e))?;
    Ok("created")
}

fn new_user(name: &str) -> NewUser {
    NewUser {
        display_name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    }
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    root: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        Self::organization(pool, "org-a").await
    }

    async fn organization(pool: PgPool, slug: &str) -> Self {
        let org = organizations::create(&pool, slug, slug, now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        Self { pool, org, root }
    }

    fn app(&self) -> Router {
        let state = AppState {
            db: self.pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
        };
        Router::new()
            .route("/gated", get(gated))
            .route("/catalog", get(catalog_gated))
            .route("/gated/{account_id}", get(gated_account))
            .route("/two", get(two_features))
            .route("/wrong-order/{account_id}", get(wrong_order))
            .route("/users", post(create_user))
            .with_state(state)
    }

    /// A signed-in user holding a custom role with `permissions` at organization scope: their
    /// `Cookie` header value.
    async fn holder(&self, permissions: &[&str]) -> String {
        let user = users::create(&self.pool, scope(self.org), &new_user("Pat Example"), now())
            .await
            .expect("create user")
            .id;
        if !permissions.is_empty() {
            let role = NewRole {
                key: format!("r{}", new_id().simple()),
                name: "Test role".into(),
                permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
            };
            let role = roles::create_custom(&self.pool, scope(self.org), &role, now())
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
            role_assignments::create(&self.pool, scope(self.org), &assignment, now())
                .await
                .expect("create assignment");
        }
        let started = session::start(&self.pool, scope(self.org), user, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        cookie.split(';').next().expect("a pair").to_owned()
    }

    /// Point the organization at a new test plan with these `features` and `limits`, by SQL.
    async fn use_plan(&self, features: &str, limits: &str) {
        let plan = new_id();
        sqlx::query(
            "INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, $3::jsonb, $4::jsonb)",
        )
        .bind(plan)
        .bind(format!("test_{}", plan.simple()))
        .bind(features)
        .bind(limits)
        .execute(&self.pool)
        .await
        .expect("insert plan");
        sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
            .bind(self.org)
            .bind(plan)
            .execute(&self.pool)
            .await
            .expect("move to the plan");
    }

    async fn users(&self) -> i64 {
        users::count_for_limit(&self.pool, scope(self.org))
            .await
            .expect("count users")
    }

    async fn call(&self, method: Method, uri: &str, cookie: Option<&str>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, self.org.to_string());
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let response = self
            .app()
            .oneshot(request.body(Body::empty()).expect("a valid request"))
            .await
            .expect("the router never fails");
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        Answer {
            status,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: String,
}

impl Answer {
    fn problem(&self) -> Problem {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("not a problem: {self:?}"))
    }

    #[track_caller]
    fn assert_refused(&self, status: StatusCode, code: &str) {
        assert_eq!(
            (self.status, self.problem().code.as_str()),
            (status, code),
            "{self:?}"
        );
    }

    #[track_caller]
    fn assert_ok(&self, body: &str) {
        assert_eq!((self.status, self.body.as_str()), (StatusCode::OK, body));
    }
}

/// The log of every SQL statement sqlx runs while the guard lives, on this thread.
#[derive(Clone, Default)]
struct Statements(Arc<Mutex<Vec<u8>>>);

impl Write for Statements {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Statements {
    fn capture() -> (Self, tracing::subscriber::DefaultGuard) {
        let statements = Self::default();
        let writer = statements.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter("sqlx::query=trace")
            .with_writer(move || writer.clone())
            .finish();
        (statements, tracing::subscriber::set_default(subscriber))
    }

    /// How many logged statements contain `needle`. sqlx logs one event per statement, and a
    /// statement's text never contains a line break here, so lines stand for statements.
    fn count(&self, needle: &str) -> usize {
        let text = String::from_utf8(self.0.lock().expect("log buffer").clone()).expect("UTF-8");
        text.lines().filter(|line| line.contains(needle)).count()
    }

    /// Fail when nothing was logged, so a test counting zero statements cannot pass because the
    /// capture missed them.
    fn assert_captured(&self) {
        assert!(self.count("sqlx::query") > 0, "no statement was logged");
    }
}

/// The entitlements query of `org_plans::get_with_plan`.
const ENTITLEMENTS_QUERY: &str = "FROM org_plans op JOIN plans p";

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_two_feature_checks_share_one_entitlements_query(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.holder(&["organization.manage"]).await;

    let (statements, guard) = Statements::capture();
    f.call(Method::GET, "/two", Some(&admin))
        .await
        .assert_ok("both");
    drop(guard);

    statements.assert_captured();
    assert_eq!(statements.count(ENTITLEMENTS_QUERY), 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_a_plan_change_shows_on_the_next_request(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let member = f.holder(&[]).await;
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_ok("gated");
    f.use_plan("{}", "{}").await;
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
    f.use_plan(r#"{"test_gate": true}"#, "{}").await;
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_ok("gated");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_unlimited_plan_passes_every_feature_check(pool: PgPool) {
    let f = Fixture::new(pool).await;
    // A member with no permission at all: the feature check needs none.
    let member = f.holder(&[]).await;
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_ok("gated");
    f.call(Method::GET, "/catalog", Some(&member))
        .await
        .assert_ok("catalog");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_test_plan_decides_each_feature(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let member = f.holder(&[]).await;
    f.use_plan(r#"{"custom_roles": true, "test_gate": false}"#, "{}")
        .await;
    f.call(Method::GET, "/catalog", Some(&member))
        .await
        .assert_ok("catalog");
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
    f.use_plan(r#"{"test_gate": true}"#, "{}").await;
    f.call(Method::GET, "/catalog", Some(&member))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_the_order_of_the_feature_check(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let existing = format!("/gated/{}", f.root);
    let missing = format!("/gated/{}", new_id());

    // Anonymous: 401 before anything else.
    f.call(Method::GET, &existing, None)
        .await
        .assert_refused(StatusCode::UNAUTHORIZED, "auth.unauthenticated");

    // Feature off: the same 403 for an id that exists and one that does not.
    let viewer = f.holder(&["account.view"]).await;
    f.use_plan("{}", "{}").await;
    for uri in [&existing, &missing] {
        f.call(Method::GET, uri, Some(&viewer))
            .await
            .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
    }
    let nothing = f.holder(&[]).await;
    f.call(Method::GET, &existing, Some(&nothing))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");

    // Feature on: the permission check runs.
    f.use_plan(r#"{"test_gate": true}"#, "{}").await;
    let others = f.holder(&["role.view"]).await;
    f.call(Method::GET, &existing, Some(&others))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    f.call(Method::GET, &missing, Some(&viewer))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    f.call(Method::GET, &existing, Some(&viewer))
        .await
        .assert_ok(&f.root.to_string());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_feature_check_after_a_permission_check_is_a_server_error(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let viewer = f.holder(&["account.view"]).await;
    f.call(
        Method::GET,
        &format!("/wrong-order/{}", f.root),
        Some(&viewer),
    )
    .await
    .assert_refused(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_organization_without_a_plan_fails_closed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let member = f.holder(&["organization.manage"]).await;
    sqlx::query("DELETE FROM org_plans WHERE org_id = $1")
        .bind(f.org)
        .execute(&f.pool)
        .await
        .unwrap();
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_refused(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
    f.call(Method::POST, "/users", Some(&member))
        .await
        .assert_refused(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_plan_that_does_not_parse_fails_closed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let member = f.holder(&[]).await;
    // An array passes neither the row's check constraint nor the reader; the constraint is
    // dropped here to plant one, as a hand-edited database could.
    sqlx::query("ALTER TABLE plans DROP CONSTRAINT plans_features_check")
        .execute(&f.pool)
        .await
        .unwrap();
    f.use_plan(r#"["test_gate"]"#, "{}").await;
    f.call(Method::GET, "/gated", Some(&member))
        .await
        .assert_refused(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_the_unlimited_plan_creates_without_counting(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.holder(&["organization.manage"]).await;

    let (statements, guard) = Statements::capture();
    let answer = f.call(Method::POST, "/users", Some(&admin)).await;
    drop(guard);

    answer.assert_ok("created");
    statements.assert_captured();
    assert_eq!(statements.count("count(*)"), 0);
    assert_eq!(statements.count("FOR UPDATE"), 0);
    assert_eq!(f.users().await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_reached_limit_refuses_and_inserts_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.holder(&["organization.manage"]).await;
    f.use_plan("{}", r#"{"users": 2}"#).await;

    f.call(Method::POST, "/users", Some(&admin))
        .await
        .assert_ok("created");
    assert_eq!(f.users().await, 2);

    let answer = f.call(Method::POST, "/users", Some(&admin)).await;
    answer.assert_refused(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    let problem = answer.problem();
    assert_eq!(problem.limit.as_deref(), Some("users"));
    assert_eq!(problem.max, Some(2));
    assert_eq!(f.users().await, 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_plan_without_a_users_limit_refuses_the_first_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.holder(&["organization.manage"]).await;
    // The admin is the organization's only user; `{}` reads as `users = 0` (B7).
    f.use_plan("{}", "{}").await;
    let answer = f.call(Method::POST, "/users", Some(&admin)).await;
    answer.assert_refused(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    assert_eq!(answer.problem().max, Some(0));
    assert_eq!(f.users().await, 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_organization_is_checked_against_its_own_plan(pool: PgPool) {
    let a = Fixture::new(pool.clone()).await;
    let b = Fixture::organization(pool, "org-b").await;
    let in_a = a.holder(&["organization.manage"]).await;
    let in_b = b.holder(&["organization.manage"]).await;
    b.use_plan("{}", r#"{"users": 1}"#).await;

    a.call(Method::GET, "/gated", Some(&in_a))
        .await
        .assert_ok("gated");
    a.call(Method::POST, "/users", Some(&in_a))
        .await
        .assert_ok("created");
    b.call(Method::GET, "/gated", Some(&in_b))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.feature_unavailable");
    b.call(Method::POST, "/users", Some(&in_b))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "entitlement.limit_reached");
    // Org A's users do not count against org B's limit, nor B's against A's.
    assert_eq!((a.users().await, b.users().await), (2, 1));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_entitlements_extractor_needs_a_principal(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.call(Method::POST, "/users", None)
        .await
        .assert_refused(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn require_for_an_unauthenticated_route(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let unlimited = Entitlements::load(&f.pool, scope(f.org)).await.unwrap();
    assert_eq!(unlimited.plan().plan, "unlimited");
    assert!(unlimited.require::<Gate>().is_ok());
    f.use_plan(r#"{"custom_roles": true}"#, "{}").await;
    let listed = Entitlements::load(&f.pool, scope(f.org)).await.unwrap();
    assert!(listed.require::<feature::CustomRoles>().is_ok());
    let refused = listed.require::<Gate>().unwrap_err();
    assert_eq!(
        (refused.status(), refused.code()),
        (StatusCode::FORBIDDEN, "entitlement.feature_unavailable")
    );
}
