// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Require<P, T>` over HTTP (docs/specs/m0/09-permissions-and-roles.md B16 to B18; T7's
//! evaluator part, T13, the four cases of B20 for each target kind). Test-only routes take each
//! target; `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).
//!
//! The organization has Tower and Enroute under the root and Ground under Tower. The request
//! organization resolver serves one organization per instance in M0 (spec 08 Q1) and refuses
//! every session while a second one exists, so the cross-organization test calls the targets
//! from a test route with org A's grants loaded directly, and org B's ids in the path or body.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode, header, request::Parts},
    routing::{get, post},
};
use http_body_util::BodyExt;
use serde::Deserialize;
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::Actor,
    clock::ManualClock,
    grants::{Grants, ScopeId},
    id::{Uuid, new_id},
    organizations,
    perm::{self, AccountManage, AccountView, OrgManage, RoleAssign, RoleView},
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole, RolePatch},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    error::{ApiError, Problem},
    extract::ApiJson,
    require::{AccountParentTarget, AccountTarget, AnyScope, Require, Target},
    session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

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

async fn org(_: Require<OrgManage>) -> &'static str {
    "org"
}

async fn view_account(require: Require<AccountView, AccountTarget>) -> String {
    require.target().id.to_string()
}

async fn manage_account(require: Require<AccountManage, AccountTarget>) -> String {
    require.target().id.to_string()
}

async fn delete_account(require: Require<AccountManage, AccountParentTarget>) -> String {
    format!("{:?}", require.target().parent_id)
}

async fn list_roles(require: Require<RoleView, AnyScope>) -> String {
    format!("{:?}", require.grants().scopes_with::<RoleView>())
}

#[derive(Deserialize)]
struct AssignBody {
    account_id: Option<Uuid>,
}

async fn assign_role(
    require: Require<RoleAssign, AnyScope>,
    axum::extract::State(state): axum::extract::State<AppState>,
    ApiJson(body): ApiJson<AssignBody>,
) -> Result<&'static str, ApiError> {
    let org = ScopeId::Organization(require.principal().scope().org_id());
    let at = body.account_id.map_or(org, ScopeId::Account);
    require
        .grants()
        .require_at::<RoleAssign>(&state.db, at)
        .await?;
    Ok("assigned")
}

async fn twice(
    view: Require<AccountView, AccountTarget>,
    manage: Require<AccountManage, AccountTarget>,
) -> String {
    format!("{}", std::ptr::eq(view.grants(), manage.grants()))
}

async fn no_param(_: Require<AccountView, AccountTarget>) -> &'static str {
    "unreachable"
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    enroute: Uuid,
}

#[derive(Clone, Copy)]
enum At {
    Org,
    Account(Uuid),
}

impl Fixture {
    /// Org A, the only organization, so sessions resolve.
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        Self::organization(pool, "org-a").await
    }

    /// A new organization `slug` with the tree.
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
        let account = |parent, name| {
            let pool = pool.clone();
            async move {
                accounts::create(&pool, scope(org), parent, name, now(), Actor::Instance)
                    .await
                    .expect("create account")
                    .id
            }
        };
        let tower = account(root, "Tower").await;
        let ground = account(tower, "Ground").await;
        let enroute = account(root, "Enroute").await;
        Self {
            pool,
            org,
            root,
            tower,
            ground,
            enroute,
        }
    }

    fn state(&self) -> AppState {
        AppState {
            db: self.pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        }
    }

    fn app(&self) -> Router {
        let routes = Router::new()
            .route("/org", get(org))
            .route(
                "/accounts/{account_id}",
                get(view_account)
                    .patch(manage_account)
                    .delete(delete_account),
            )
            .route("/twice/{account_id}", get(twice))
            .route("/roles", get(list_roles))
            .route("/assignments", post(assign_role))
            .route("/no-param", get(no_param));
        self.routes(routes)
    }

    fn routes(&self, router: Router<AppState>) -> Router {
        router.with_state(self.state())
    }

    /// A new user, signed in, with no assignments: their `Cookie` header value and id.
    async fn user(&self) -> (String, Uuid) {
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
        let id = users::create(&self.pool, scope(self.org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(self.org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    async fn custom_role(&self, permissions: &[&str]) -> Uuid {
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    async fn builtin(&self, key: &str) -> Uuid {
        sqlx::query_scalar("SELECT id FROM roles WHERE org_id IS NULL AND key = $1")
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .expect("a built-in role")
    }

    async fn assign(&self, user: Uuid, role: Uuid, at: At) -> Uuid {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: match at {
                At::Org => Scope::Organization,
                At::Account(id) => Scope::Account(id),
            },
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
        .expect("create assignment")
        .id
    }

    /// A signed-in user holding a custom role with `permissions` at `at`.
    async fn holder(&self, permissions: &[&str], at: At) -> String {
        let (cookie, user) = self.user().await;
        let role = self.custom_role(permissions).await;
        self.assign(user, role, at).await;
        cookie
    }

    async fn holder_of_builtin(&self, key: &str, at: At) -> String {
        let (cookie, user) = self.user().await;
        self.assign(user, self.builtin(key).await, at).await;
        cookie
    }

    async fn call(&self, method: Method, uri: &str, cookie: Option<&str>) -> Answer {
        self.send(method, uri, cookie, None).await
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        cookie: Option<&str>,
        json: Option<String>,
    ) -> Answer {
        self.send_to(self.app(), method, uri, cookie, json).await
    }

    async fn send_to(
        &self,
        app: Router,
        method: Method,
        uri: impl AsRef<str>,
        cookie: Option<&str>,
        json: Option<String>,
    ) -> Answer {
        let mut request = Request::builder().method(method).uri(uri.as_ref());
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match json {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json)
            }
            None => Body::empty(),
        };
        let response = app
            .oneshot(request.body(body).expect("a valid request"))
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

    fn account_uri(&self, id: Uuid) -> String {
        format!("/accounts/{id}")
    }
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    body: String,
}

impl Answer {
    fn code(&self) -> String {
        serde_json::from_str::<Problem>(&self.body)
            .unwrap_or_else(|_| panic!("not a problem: {}", self.body))
            .code
    }

    #[track_caller]
    fn assert_refused(&self, status: StatusCode, code: &str) {
        assert_eq!(
            (self.status, self.code().as_str()),
            (status, code),
            "{self:?}"
        );
    }

    #[track_caller]
    fn assert_ok(&self, body: &str) {
        assert_eq!((self.status, self.body.as_str()), (StatusCode::OK, body));
    }
}

/// Every catalog key but `except`.
fn all_but(except: &str) -> Vec<&'static str> {
    perm::ALL
        .iter()
        .map(|p| p.key())
        .filter(|&k| k != except)
        .collect()
}

fn all() -> Vec<&'static str> {
    all_but("")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn every_target_answers_401_without_a_session(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower = f.account_uri(f.tower);
    for (method, uri) in [
        (Method::GET, "/org"),
        (Method::GET, tower.as_str()),
        (Method::PATCH, tower.as_str()),
        (Method::DELETE, tower.as_str()),
        (Method::GET, "/roles"),
        // Authentication comes before the path parameter is read.
        (Method::GET, "/accounts/not-a-uuid"),
    ] {
        f.call(method, uri, None)
            .await
            .assert_refused(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
    }
    let unknown = format!("{}=AAAA", session::COOKIE_NAME);
    f.call(Method::GET, "/org", Some(&unknown))
        .await
        .assert_refused(StatusCode::UNAUTHORIZED, "auth.unauthenticated");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_session_extractor_keeps_its_specific_code(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, user) = f.user().await;
    f.assign(user, f.builtin("org_admin").await, At::Org).await;
    // A second active admin, so suspending the first does not remove the last one (B14).
    let (_, other) = f.user().await;
    f.assign(other, f.builtin("org_admin").await, At::Org).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        user,
        users::Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    f.call(Method::GET, "/org", Some(&cookie))
        .await
        .assert_refused(StatusCode::UNAUTHORIZED, "auth.suspended");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn org_target_four_cases(pool: PgPool) {
    let f = Fixture::new(pool).await;
    // 403: every other permission at the organization, so it is visible.
    let others = f.holder(&all_but("organization.manage"), At::Org).await;
    f.call(Method::GET, "/org", Some(&others))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    // A member with nothing still sees the organization: 403, not 404.
    let (nothing, _) = f.user().await;
    f.call(Method::GET, "/org", Some(&nothing))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    // Organization admin at the root account does not get organization-only permissions.
    let root_admin = f.holder_of_builtin("org_admin", At::Account(f.root)).await;
    f.call(Method::GET, "/org", Some(&root_admin))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    // 2xx: exactly the permission.
    let exact = f.holder(&["organization.manage"], At::Org).await;
    f.call(Method::GET, "/org", Some(&exact))
        .await
        .assert_ok("org");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn account_target_four_cases(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower = f.account_uri(f.tower);
    let others = f
        .holder(&all_but("account.manage"), At::Account(f.tower))
        .await;
    f.call(Method::PATCH, &tower, Some(&others))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    let sibling = f.holder(&all(), At::Account(f.enroute)).await;
    f.call(Method::PATCH, &tower, Some(&sibling))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    let exact = f.holder(&["account.manage"], At::Account(f.tower)).await;
    f.call(Method::PATCH, &tower, Some(&exact))
        .await
        .assert_ok(&f.tower.to_string());
    // The grant at Tower covers Ground.
    f.call(Method::PATCH, &f.account_uri(f.ground), Some(&exact))
        .await
        .assert_ok(&f.ground.to_string());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_the_order_of_the_mapping(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower = f.account_uri(f.tower);
    let (nothing, _) = f.user().await;
    f.call(Method::GET, &tower, Some(&nothing))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    let ground_only = f.holder(&["account.view"], At::Account(f.ground)).await;
    f.call(Method::GET, &tower, Some(&ground_only))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    let other_at_root = f.holder(&["role.view"], At::Account(f.root)).await;
    f.call(Method::GET, &tower, Some(&other_at_root))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unknown_or_malformed_account_id(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f.holder_of_builtin("org_admin", At::Org).await;
    f.call(Method::GET, &f.account_uri(new_id()), Some(&admin))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    f.call(Method::DELETE, &f.account_uri(new_id()), Some(&admin))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    f.call(Method::GET, "/accounts/not-a-uuid", Some(&admin))
        .await
        .assert_refused(StatusCode::BAD_REQUEST, "request.malformed");
    f.call(Method::GET, "/no-param", Some(&admin))
        .await
        .assert_refused(StatusCode::INTERNAL_SERVER_ERROR, "internal.error");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn account_parent_target_checks_the_parent(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let tower_admin = f
        .holder_of_builtin("account_admin", At::Account(f.tower))
        .await;
    // Tower's parent is the root, which a Tower admin cannot see.
    f.call(Method::DELETE, &f.account_uri(f.tower), Some(&tower_admin))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    f.call(Method::DELETE, &f.account_uri(f.ground), Some(&tower_admin))
        .await
        .assert_ok(&format!("{:?}", Some(f.tower)));
    // Visible parent without the permission.
    let viewer = f
        .holder(&all_but("account.manage"), At::Account(f.tower))
        .await;
    f.call(Method::DELETE, &f.account_uri(f.ground), Some(&viewer))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    let sibling = f.holder(&all(), At::Account(f.enroute)).await;
    f.call(Method::DELETE, &f.account_uri(f.ground), Some(&sibling))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_root_parent_is_reached_only_with_the_permission_at_organization_scope(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let root = f.account_uri(f.root);
    for denied in [
        f.holder(&all(), At::Account(f.root)).await,
        f.holder(&all_but("account.manage"), At::Org).await,
    ] {
        f.call(Method::DELETE, &root, Some(&denied))
            .await
            .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    }
    let org_manager = f.holder(&["account.manage"], At::Org).await;
    f.call(Method::DELETE, &root, Some(&org_manager))
        .await
        .assert_ok("None");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn any_scope_needs_the_permission_somewhere(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let others = f.holder(&all_but("role.view"), At::Org).await;
    f.call(Method::GET, "/roles", Some(&others))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    let (nothing, _) = f.user().await;
    f.call(Method::GET, "/roles", Some(&nothing))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
    let at_tower = f.holder(&["role.view"], At::Account(f.tower)).await;
    f.call(Method::GET, "/roles", Some(&at_tower))
        .await
        .assert_ok(&format!("[Account({})]", f.tower));
    let at_org = f.holder(&["role.view"], At::Org).await;
    f.call(Method::GET, "/roles", Some(&at_org))
        .await
        .assert_ok(&format!("[Organization({})]", f.org));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_target_is_checked_with_the_same_mapping(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let assigner = f.holder(&["role.assign"], At::Account(f.tower)).await;
    let body = |id: Option<Uuid>| Some(serde_json::json!({ "account_id": id }).to_string());
    let post = |id| f.send(Method::POST, "/assignments", Some(&assigner), body(id));
    post(Some(f.ground)).await.assert_ok("assigned");
    post(Some(f.enroute))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    post(Some(new_id()))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    post(None)
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_requires_in_one_request_share_one_evaluation(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let admin = f
        .holder_of_builtin("account_admin", At::Account(f.tower))
        .await;
    f.call(Method::GET, &format!("/twice/{}", f.tower), Some(&admin))
        .await
        .assert_ok("true");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_role_change_applies_to_the_next_request(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, user) = f.user().await;
    let role = f.custom_role(&["account.view", "account.manage"]).await;
    let assignment = f.assign(user, role, At::Account(f.tower)).await;
    let tower = f.account_uri(f.tower);
    f.call(Method::PATCH, &tower, Some(&cookie))
        .await
        .assert_ok(&f.tower.to_string());

    let patch = RolePatch {
        name: None,
        permissions: Some(vec!["account.view".into()]),
    };
    roles::update_custom(&f.pool, scope(f.org), role, &patch, now(), Actor::Instance)
        .await
        .expect("update role");
    f.call(Method::PATCH, &tower, Some(&cookie))
        .await
        .assert_refused(StatusCode::FORBIDDEN, "permission.denied");

    role_assignments::delete(
        &f.pool,
        scope(f.org),
        assignment,
        UNIX_EPOCH,
        Actor::Instance,
    )
    .await
    .expect("delete assignment");
    f.call(Method::GET, &tower, Some(&cookie))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
}

/// What a target or `require_at` answered, as the handler of a protected route would.
fn answer<T>(result: Result<T, ApiError>) -> Result<&'static str, ApiError> {
    result.map(|_| "passed")
}

#[derive(Deserialize)]
struct AtBody {
    organization_id: Option<Uuid>,
    account_id: Option<Uuid>,
}

async fn probe_account(
    state: AppState,
    mut parts: Parts,
    grants: Grants,
) -> Result<&'static str, ApiError> {
    answer(AccountTarget::resolve::<AccountView>(&mut parts, &state, &grants).await)
}

async fn probe_parent(
    state: AppState,
    mut parts: Parts,
    grants: Grants,
) -> Result<&'static str, ApiError> {
    answer(AccountParentTarget::resolve::<AccountManage>(&mut parts, &state, &grants).await)
}

async fn probe_at(state: AppState, body: AtBody, grants: Grants) -> Result<&'static str, ApiError> {
    let at = match (body.organization_id, body.account_id) {
        (Some(org), _) => ScopeId::Organization(org),
        (None, Some(account)) => ScopeId::Account(account),
        (None, None) => panic!("the test names a target"),
    };
    answer(
        grants
            .require_at::<RoleAssign>(&state.db, at)
            .await
            .map_err(ApiError::from),
    )
}

/// Routes that run each target, and `require_at`, with fixed `grants` in place of the session
/// and evaluation steps of `Require`.
fn probe_routes(grants: Grants) -> Router<AppState> {
    let (account, parent, at) = (grants.clone(), grants.clone(), grants);
    Router::new()
        .route(
            "/probe/account/{account_id}",
            get(move |State(state): State<AppState>, parts: Parts| {
                probe_account(state, parts, account)
            }),
        )
        .route(
            "/probe/parent/{account_id}",
            get(move |State(state): State<AppState>, parts: Parts| {
                probe_parent(state, parts, parent)
            }),
        )
        .route(
            "/probe/at",
            post(
                move |State(state): State<AppState>, ApiJson(body): ApiJson<AtBody>| {
                    probe_at(state, body, at)
                },
            ),
        )
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_ids_are_not_found_for_its_admin(pool: PgPool) {
    let a = Fixture::new(pool.clone()).await;
    let (_, admin) = a.user().await;
    a.assign(admin, a.builtin("org_admin").await, At::Org).await;
    let b = Fixture::organization(pool.clone(), "org-b").await;
    let grants = Grants::load(&pool, scope(a.org), admin)
        .await
        .expect("load grants");
    let app = a.routes(probe_routes(grants));
    let get = |uri: String| a.send_to(app.clone(), Method::GET, uri, None, None);
    let at = |body: serde_json::Value| {
        let uri = "/probe/at".to_owned();
        a.send_to(app.clone(), Method::POST, uri, None, Some(body.to_string()))
    };

    // Org A's own accounts pass, so the probe reaches the check.
    get(format!("/probe/account/{}", a.tower))
        .await
        .assert_ok("passed");
    get(format!("/probe/parent/{}", a.ground))
        .await
        .assert_ok("passed");
    for id in [b.root, b.tower, b.ground] {
        get(format!("/probe/account/{id}"))
            .await
            .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
        get(format!("/probe/parent/{id}"))
            .await
            .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    }

    at(serde_json::json!({ "account_id": a.tower }))
        .await
        .assert_ok("passed");
    at(serde_json::json!({ "organization_id": a.org }))
        .await
        .assert_ok("passed");
    at(serde_json::json!({ "account_id": b.tower }))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "account.not_found");
    at(serde_json::json!({ "organization_id": b.org }))
        .await
        .assert_refused(StatusCode::NOT_FOUND, "org.not_found");
}
