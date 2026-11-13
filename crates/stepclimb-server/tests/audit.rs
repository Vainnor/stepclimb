// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audit entries written by the routes over the whole router
//! (docs/specs/m0/09-permissions-and-roles.md Q5 and "Audit entries"): each change a route
//! makes writes exactly one entry naming the signed-in user and the client address the rate
//! limits count by, and a refused request writes none. The repository side (data before and
//! after, rollback, organization scope) is in `stepclimb-core`'s `tests/audit.rs`.
//!
//! One organization, so the production request-organization resolver serves it.
//! `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Router,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor, Entry},
    clock::ManualClock,
    id::Uuid,
    organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState,
    client_ip::TrustedProxies,
    rate_limit::{Limits, RateLimiter},
    router, session,
    web_origin::WebOrigin,
};
use tower::ServiceExt;

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

/// The TCP peer of every request (TEST-NET-1).
const PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the sign-in specs and the admin routes that build these rows"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Fixture {
    pool: PgPool,
    app: Router,
    org: Uuid,
    root: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool, trusted: TrustedProxies) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let org = organizations::create(&pool, "org-a", "Org A", now())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .expect("the root");
        let app = router(AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: RateLimiter::new(Limits::DEFAULT, trusted),
        })
        .layer(Extension(ConnectInfo(SocketAddr::new(PEER, 50_000))));
        Self {
            pool,
            app,
            org,
            root,
        }
    }

    /// A new signed-in user holding `role` at organization scope: their `Cookie` value and id.
    async fn holder_of(&self, role: Uuid) -> (String, Uuid) {
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
        let s = scope(self.org);
        let id = users::create(&self.pool, s, &new_user, now())
            .await
            .expect("create user")
            .id;
        self.assign(id, role).await;
        let started = session::start(&self.pool, s, id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    async fn assign(&self, user: Uuid, role: Uuid) -> Uuid {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: Scope::Organization,
            source: Source::Local,
            external_id: None,
        };
        let s = scope(self.org);
        role_assignments::create(&self.pool, s, &assignment, now(), Actor::Instance)
            .await
            .expect("create assignment")
            .id
    }

    async fn builtin(&self, key: &str) -> Uuid {
        roles::builtin_id(&self.pool, key)
            .await
            .expect("read the role")
            .expect("synced")
    }

    async fn custom_role(&self, key: &str, permissions: &[&str]) -> Uuid {
        let role = NewRole {
            key: key.into(),
            name: format!("Role {key}"),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        roles::create_custom(&self.pool, scope(self.org), &role, now(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }

    async fn entries(&self) -> Vec<Entry> {
        audit::list(&self.pool, scope(self.org), 1000)
            .await
            .expect("list entries")
    }

    /// Send a request as `cookie`; the status and the JSON body (`Null` when empty).
    async fn send(
        &self,
        method: Method,
        uri: &str,
        cookie: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header("sec-fetch-site", "same-origin");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).expect("a valid request"))
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
        (status, body)
    }
}

fn id_of(body: &Value) -> Uuid {
    body["id"].as_str().expect("an id").parse().expect("a UUID")
}

/// One change through the router: what to send, and which entry it must write.
struct Step {
    method: Method,
    uri: String,
    body: Option<Value>,
    status: StatusCode,
    action: &'static str,
    /// `None`: the id the response body names.
    target: Option<Uuid>,
}

impl Step {
    fn new(method: Method, uri: String, body: Option<Value>, status: StatusCode) -> Self {
        Self {
            method,
            uri,
            body,
            status,
            action: "",
            target: None,
        }
    }

    fn writes(mut self, action: &'static str, target: Option<Uuid>) -> Self {
        self.action = action;
        self.target = target;
        self
    }
}

/// Run `step` as the admin `(cookie, id)` and check it wrote exactly one entry by the admin
/// from [`PEER`]; the id the response names, if any.
async fn run(f: &Fixture, admin: &(String, Uuid), step: Step) -> Option<Uuid> {
    let before = f.entries().await.len();
    let (status, body) = f
        .send(step.method, &step.uri, &admin.0, step.body, &[])
        .await;
    assert_eq!(status, step.status, "{} {body}", step.uri);
    let created = body.get("id").map(|_| id_of(&body));
    let entries = f.entries().await;
    let new = &entries[before..];
    assert_eq!(new.len(), 1, "{}: {new:#?}", step.action);
    assert_eq!(new[0].action, step.action);
    let actor = Actor::User {
        id: admin.1,
        ip: PEER,
    };
    assert_eq!(new[0].actor, actor, "{}", step.action);
    let target = step.target.or(created).expect("a target");
    assert_eq!(new[0].target_id, target, "{}", step.action);
    created
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_change_through_a_route_writes_one_entry_by_its_user(pool: PgPool) {
    let f = Fixture::new(pool, TrustedProxies::none()).await;
    let admin = f.holder_of(f.builtin("org_admin").await).await;
    let (ok, created) = (StatusCode::OK, StatusCode::CREATED);
    let gone = StatusCode::NO_CONTENT;
    let roles_uri = "/api/v1/admin/roles".to_owned();
    let accounts_uri = "/api/v1/accounts".to_owned();

    let body = json!({ "key": "mentor", "name": "Mentor", "permissions": ["account.view"] });
    let step = Step::new(Method::POST, roles_uri.clone(), Some(body), created);
    let role = run(&f, &admin, step.writes("role.create", None)).await;
    let role = role.expect("the role's id");
    let uri = format!("{roles_uri}/{role}");
    let step = Step::new(Method::PATCH, uri.clone(), Some(json!({ "name": "M" })), ok);
    run(&f, &admin, step.writes("role.update", Some(role))).await;

    let user = f.holder_of(f.custom_role("none", &[]).await).await.1;
    let scope = json!({ "type": "organization", "id": f.org });
    let body = json!({ "user_id": user, "role_id": role, "scope": scope });
    let assignments = "/api/v1/admin/role-assignments".to_owned();
    let step = Step::new(Method::POST, assignments.clone(), Some(body), created);
    let assignment = run(&f, &admin, step.writes("role_assignment.grant", None)).await;
    let assignment = assignment.expect("the assignment's id");
    let step = Step::new(
        Method::DELETE,
        format!("{assignments}/{assignment}"),
        None,
        gone,
    );
    run(
        &f,
        &admin,
        step.writes("role_assignment.remove", Some(assignment)),
    )
    .await;
    let step = Step::new(Method::DELETE, uri, None, gone);
    run(&f, &admin, step.writes("role.delete", Some(role))).await;

    let body = json!({ "parent_id": f.root, "name": "Tower" });
    let step = Step::new(Method::POST, accounts_uri.clone(), Some(body), created);
    let tower = run(&f, &admin, step.writes("account.create", None)).await;
    let tower = tower.expect("Tower's id");
    let body = json!({ "parent_id": f.root, "name": "Ground" });
    let step = Step::new(Method::POST, accounts_uri.clone(), Some(body), created);
    let ground = run(&f, &admin, step.writes("account.create", None)).await;
    let ground = ground.expect("Ground's id");
    let uri = format!("{accounts_uri}/{ground}");
    let step = Step::new(Method::PATCH, uri.clone(), Some(json!({ "name": "G" })), ok);
    run(&f, &admin, step.writes("account.rename", Some(ground))).await;
    let body = json!({ "parent_id": tower });
    let step = Step::new(Method::POST, format!("{uri}/move"), Some(body), ok);
    run(&f, &admin, step.writes("account.move", Some(ground))).await;
    let step = Step::new(Method::DELETE, uri, None, gone);
    run(&f, &admin, step.writes("account.delete", Some(ground))).await;

    let body = json!({ "name": "Renamed" });
    let step = Step::new(Method::PATCH, "/api/v1/organization".into(), Some(body), ok);
    run(&f, &admin, step.writes("organization.rename", Some(f.org))).await;
}

/// A patch that names nothing, or only the values the row already has, changes nothing and
/// writes nothing; neither does a move to the current parent.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_request_that_changes_nothing_writes_no_entry(pool: PgPool) {
    let f = Fixture::new(pool, TrustedProxies::none()).await;
    let (cookie, _) = f.holder_of(f.builtin("org_admin").await).await;
    let tower = organizations_child(&f, &cookie, "Tower").await;
    let role = f
        .custom_role("mentor", &["account.view", "user.view"])
        .await;
    let before = f.entries().await.len();

    let tower_uri = format!("/api/v1/accounts/{tower}");
    let move_uri = format!("{tower_uri}/move");
    let role_uri = format!("/api/v1/admin/roles/{role}");
    let same_role = json!({ "name": "Role mentor", "permissions": ["user.view", "account.view"] });
    let calls = [
        (Method::PATCH, role_uri.clone(), json!({})),
        (Method::PATCH, role_uri, same_role),
        (
            Method::PATCH,
            tower_uri.clone(),
            json!({ "name": " Tower " }),
        ),
        (
            Method::PATCH,
            "/api/v1/organization".to_owned(),
            json!({ "name": "Org A" }),
        ),
        (Method::PATCH, "/api/v1/organization".to_owned(), json!({})),
        (
            Method::PATCH,
            "/api/v1/organization/settings".to_owned(),
            json!({}),
        ),
        (Method::PATCH, tower_uri, json!({})),
        (Method::POST, move_uri, json!({ "parent_id": f.root })),
    ];
    for (method, uri, body) in calls {
        let (status, answer) = f.send(method, &uri, &cookie, Some(body), &[]).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
    }

    assert_eq!(f.entries().await.len(), before);
}

async fn organizations_child(f: &Fixture, cookie: &str, name: &str) -> Uuid {
    let body = json!({ "parent_id": f.root, "name": name });
    let (status, answer) = f
        .send(Method::POST, "/api/v1/accounts", cookie, Some(body), &[])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    id_of(&answer)
}

/// Refused before the repository (403 permission, 403 escalation) and by it (409, 422): no
/// entry either way.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_request_writes_no_entry(pool: PgPool) {
    let f = Fixture::new(pool, TrustedProxies::none()).await;
    let (admin, _) = f.holder_of(f.builtin("org_admin").await).await;
    let manager = f
        .custom_role("roles_only", &["role.manage", "role.view"])
        .await;
    let (limited, _) = f.holder_of(manager).await;
    let (nobody, _) = f.holder_of(f.custom_role("nobody", &[]).await).await;
    let before = f.entries().await.len();

    let escalating = json!({ "key": "big", "name": "Big", "permissions": ["user.manage"] });
    let taken = json!({ "key": "nobody", "name": "Again", "permissions": [] });
    let blank = json!({ "parent_id": f.root, "name": " " });
    let calls = [
        (
            &nobody,
            Method::POST,
            "/api/v1/admin/roles",
            escalating.clone(),
            403,
        ),
        (
            &limited,
            Method::POST,
            "/api/v1/admin/roles",
            escalating,
            403,
        ),
        (&admin, Method::POST, "/api/v1/admin/roles", taken, 409),
        (&admin, Method::POST, "/api/v1/accounts", blank, 422),
        (
            &nobody,
            Method::PATCH,
            "/api/v1/organization",
            json!({ "name": "X" }),
            403,
        ),
    ];
    for (cookie, method, uri, body, status) in calls {
        let (got, answer) = f.send(method, uri, cookie, Some(body), &[]).await;
        assert_eq!(got.as_u16(), status, "{uri}: {answer}");
    }
    let root = format!("/api/v1/accounts/{}", f.root);
    let (got, answer) = f.send(Method::DELETE, &root, &admin, None, &[]).await;
    assert_eq!(got, StatusCode::CONFLICT, "{answer}");

    assert_eq!(f.entries().await.len(), before);
}

/// The address is the rate limits' client address: the peer, unless the peer is a trusted
/// proxy, when it is the right-most `X-Forwarded-For` entry that is not one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_entry_records_the_client_address_behind_a_trusted_proxy(pool: PgPool) {
    let trusted = TrustedProxies::parse("192.0.2.10").expect("a valid list");
    let f = Fixture::new(pool, trusted).await;
    let (cookie, admin) = f.holder_of(f.builtin("org_admin").await).await;
    let forwarded = [("x-forwarded-for", "203.0.113.9, 198.51.100.4")];
    let body = json!({ "name": "Renamed" });

    let (status, answer) = f
        .send(
            Method::PATCH,
            "/api/v1/organization",
            &cookie,
            Some(body),
            &forwarded,
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{answer}");
    let entries = f.entries().await;
    let last = entries.last().expect("an entry");
    let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
    assert_eq!(last.actor, Actor::User { id: admin, ip });
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_untrusted_peers_forwarded_header_is_not_recorded(pool: PgPool) {
    let f = Fixture::new(pool, TrustedProxies::none()).await;
    let (cookie, admin) = f.holder_of(f.builtin("org_admin").await).await;
    let forwarded = [("x-forwarded-for", "198.51.100.4")];
    let body = json!({ "name": "Renamed" });

    let (status, answer) = f
        .send(
            Method::PATCH,
            "/api/v1/organization",
            &cookie,
            Some(body),
            &forwarded,
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{answer}");
    let entries = f.entries().await;
    let last = entries.last().expect("an entry");
    assert_eq!(
        last.actor,
        Actor::User {
            id: admin,
            ip: PEER
        }
    );
}

/// A route that would write an entry refuses to run without the peer address instead of
/// writing an entry it cannot place.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_change_without_the_peer_address_is_refused(pool: PgPool) {
    let f = Fixture::new(pool, TrustedProxies::none()).await;
    let (cookie, _) = f.holder_of(f.builtin("org_admin").await).await;
    let before = f.entries().await.len();
    let state = AppState {
        db: f.pool.clone(),
        clock: ManualClock::new(now()).clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
    };
    let request = Request::patch("/api/v1/organization")
        .header(header::COOKIE, &cookie)
        .header("sec-fetch-site", "same-origin")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"Renamed"}"#))
        .expect("a valid request");

    let response = router(state).oneshot(request).await.expect("an answer");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(f.entries().await.len(), before);
    let name = organizations::get(&f.pool, scope(f.org))
        .await
        .unwrap()
        .name;
    assert_eq!(name, "Org A");
}
