// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity routes over the whole router (docs/specs/m0/03-users-and-identities.md B18,
//! B19, B23, T15, T16, T18, T20's identities part) and the escalation rule on them. The four
//! permission cases and the cross-organization case of each route are in
//! `tests/permissions.rs`.
//!
//! Org A holds Tower under the root and two identity providers; org B exists to hold ids and a
//! provider of another organization, so requests name org A with the `test-support` header.
//! `session::start` stands in for the sign-in specs. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    OrgScope, Source, accounts,
    audit::{self, Actor},
    clock::ManualClock,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers, organizations,
    role_assignments::{self, NewAssignment, Scope},
    roles::{self, NewRole},
    users::{self, NewUser},
};
use stepclimb_server::{
    AppState, openapi, request_org::TEST_ORG_HEADER, router, session, web_origin::WebOrigin,
};
use tower::ServiceExt;

mod common;

/// The client address every test request comes from (TEST-NET-1).
const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);

const MY_IDENTITIES: &str = "/api/v1/me/identities";

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

fn user_identities(user: Uuid) -> String {
    format!("/api/v1/users/{user}/identities")
}

/// One organization's ids.
struct Org {
    id: Uuid,
    tower: Uuid,
    /// Key `example-oidc`, labeled "Sign in with Example".
    oidc: Uuid,
    /// Key `example-oauth`, with no label in its config.
    oauth: Uuid,
}

struct Fixture {
    pool: PgPool,
    app: Router,
    a: Org,
    b: Org,
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

    /// The `(field, code)` pairs of a 422's `errors`.
    fn errors(&self) -> Vec<(String, String)> {
        self.body["errors"]
            .as_array()
            .map(|errors| {
                errors
                    .iter()
                    .map(|e| {
                        (
                            e["field"].as_str().expect("a field").into(),
                            e["code"].as_str().expect("a code").into(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

async fn provider(pool: &PgPool, org: Uuid, key: &str, config: Value) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, $3, 'oidc', $4::text::jsonb)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .bind(config.to_string())
    .execute(pool)
    .await
    .expect("insert provider");
    id
}

async fn org(pool: &PgPool, slug: &str) -> Org {
    let id = organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id;
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("the root");
    let tower = accounts::create(pool, scope(id), root, "Tower", now(), Actor::Instance)
        .await
        .expect("create account")
        .id;
    let label = json!({ "label": "Sign in with Example", "client_secret": "sealed" });
    Org {
        id,
        tower,
        oidc: provider(pool, id, "example-oidc", label).await,
        oauth: provider(pool, id, "example-oauth", json!({ "client_id": "x" })).await,
    }
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        roles::sync_builtin(&pool, now()).await.expect("sync roles");
        let a = org(&pool, "org-a").await;
        let b = org(&pool, "org-b").await;
        let app = router(AppState {
            db: pool.clone(),
            clock: ManualClock::new(now()).clock(),
            keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
                .expect("a valid test key"),
            public_url: WebOrigin::parse("https://lms.example.org").expect("a valid origin"),
            allowed_origins: Vec::new(),
            rate_limits: Default::default(),
            passwords: Default::default(),
            storage: None,
            providers: stepclimb_server::providers::Providers::new(false)
                .expect("provider clients"),
        })
        .layer(Extension(ConnectInfo(CLIENT)));
        Self { pool, app, a, b }
    }

    /// A new user of `org` from `source`, signed in: their `Cookie` value and id.
    async fn user_in(&self, org: Uuid, source: Source) -> (String, Uuid) {
        let new_user = NewUser {
            display_name: "Pat Example".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source,
            external_id: None,
        };
        let id = users::create(&self.pool, scope(org), &new_user, now())
            .await
            .expect("create user")
            .id;
        let started = session::start(&self.pool, scope(org), id, &HeaderMap::new(), now())
            .await
            .expect("start a session");
        let cookie = started.set_cookie.to_str().expect("ASCII");
        (cookie.split(';').next().expect("a pair").to_owned(), id)
    }

    /// A new `local` user of org A, signed in.
    async fn user(&self) -> (String, Uuid) {
        self.user_in(self.a.id, Source::Local).await
    }

    async fn assign(&self, user: Uuid, role: Uuid, scope_: Scope) {
        let assignment = NewAssignment {
            user_id: user,
            role_id: role,
            scope: scope_,
            source: Source::Local,
            external_id: None,
        };
        role_assignments::create(
            &self.pool,
            scope(self.a.id),
            &assignment,
            now(),
            Actor::Instance,
        )
        .await
        .expect("create assignment");
    }

    async fn builtin(&self, key: &str) -> Uuid {
        roles::builtin_id(&self.pool, key)
            .await
            .expect("read role")
            .expect("a built-in role")
    }

    /// A signed-in user of org A holding the built-in role `key` at the organization.
    async fn admin(&self) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        self.assign(user, self.builtin("org_admin").await, Scope::Organization)
            .await;
        (cookie, user)
    }

    /// A signed-in user of org A holding a custom role with exactly `permissions` at the
    /// organization.
    async fn holder(&self, permissions: &[&str]) -> (String, Uuid) {
        let (cookie, user) = self.user().await;
        let role = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Test role".into(),
            permissions: permissions.iter().map(|&p| p.to_owned()).collect(),
        };
        let role =
            roles::create_custom(&self.pool, scope(self.a.id), &role, now(), Actor::Instance)
                .await
                .expect("create role")
                .id;
        self.assign(user, role, Scope::Organization).await;
        (cookie, user)
    }

    /// Link `user` of `org` to `provider`'s `subject` with `attributes`; the identity's id.
    async fn link(
        &self,
        org: Uuid,
        user: Uuid,
        provider: Uuid,
        subject: &str,
        attributes: Value,
    ) -> Uuid {
        let Value::Object(attributes) = attributes else {
            panic!("attributes are an object");
        };
        let identity = NewIdentity {
            user_id: user,
            provider_id: provider,
            subject: subject.into(),
            attributes,
        };
        identities::link(&self.pool, scope(org), &identity, now(), Actor::Instance)
            .await
            .expect("link identity")
            .id
    }

    async fn send(&self, method: Method, uri: &str, cookie: &str, body: Option<Value>) -> Answer {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(TEST_ORG_HEADER, self.a.id.to_string())
            .header(header::COOKIE, cookie)
            .header("sec-fetch-site", "same-origin");
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
        Answer { status, body }
    }

    async fn post(&self, cookie: &str, user: Uuid, key: &str, subject: &str) -> Answer {
        let body = json!({ "provider_key": key, "subject": subject });
        self.send(Method::POST, &user_identities(user), cookie, Some(body))
            .await
    }

    async fn delete(&self, cookie: &str, user: Uuid, identity: Uuid) -> Answer {
        let uri = format!("{}/{identity}", user_identities(user));
        self.send(Method::DELETE, &uri, cookie, None).await
    }

    async fn delete_mine(&self, cookie: &str, identity: Uuid) -> Answer {
        let uri = format!("{MY_IDENTITIES}/{identity}");
        self.send(Method::DELETE, &uri, cookie, None).await
    }

    /// Every `identities` row as text, in id order.
    async fn table(&self) -> Vec<String> {
        sqlx::query_scalar("SELECT to_jsonb(i)::text FROM identities i ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .expect("read identities")
    }
}

// Linking (B18, T15).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_pre_provisioned_identity_signs_its_user_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, user) = f.user().await;

    let created = f.post(&admin, user, "example-oidc", "1000001").await;

    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    assert_eq!(created.body["provider_key"], "example-oidc");
    assert_eq!(created.body["subject"], "1000001");
    assert_eq!(created.body["attributes"], json!({}));
    assert_eq!(created.body["created_at"], "2025-11-13T02:13:20.000000Z");
    let found = identities::find_user(&f.pool, scope(f.a.id), f.a.oidc, "1000001")
        .await
        .unwrap()
        .expect("a linked user");
    assert_eq!(found.user.id, user);
    assert_eq!(found.identity_id.to_string(), created.body["id"]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_an_unknown_or_other_organizations_provider_key_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, user) = f.user().await;
    // Org B has its own provider under a key org A lacks.
    provider(&f.pool, f.b.id, "only-b", json!({})).await;

    // `local` is org A's local provider, which takes no pre-provisioned identity (spec 04 B3).
    for key in ["missing", "only-b", "local", "nul\u{0}"] {
        let refused = f.post(&admin, user, key, "1000001").await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{key}");
        let code = if key.contains('\0') {
            "text.contains_nul"
        } else {
            "identity.unknown_provider"
        };
        assert_eq!(
            refused.errors(),
            [("/provider_key".to_owned(), code.to_owned())],
            "{key}"
        );
    }
    let empty = f.post(&admin, user, "example-oidc", "").await;
    assert_eq!(
        empty.errors(),
        [("/subject".to_owned(), "identity.subject_empty".to_owned())]
    );
    let long = f.post(&admin, user, "example-oidc", &"9".repeat(256)).await;
    assert_eq!(
        long.errors(),
        [("/subject".to_owned(), "text.too_long".to_owned())]
    );
    let longest = f.post(&admin, user, "example-oidc", &"9".repeat(255)).await;
    assert_eq!(longest.status, StatusCode::CREATED, "{longest:?}");
    // Only the 255-character subject was stored.
    assert_eq!(f.table().await.len(), 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_subject_linked_to_anyone_is_taken(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, sam) = f.user().await;
    let (_, pat) = f.user().await;
    f.link(f.a.id, sam, f.a.oidc, "1000001", json!({})).await;
    let before = f.table().await;

    let to_pat = f.post(&admin, pat, "example-oidc", "1000001").await;
    let to_sam = f.post(&admin, sam, "example-oidc", "1000001").await;

    for answer in [to_pat, to_sam] {
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::CONFLICT, "identity.taken")
        );
    }
    assert_eq!(f.table().await, before);
    // The same subject at another provider is another identity.
    let other = f.post(&admin, pat, "example-oauth", "1000001").await;
    assert_eq!(other.status, StatusCode::CREATED, "{other:?}");
}

/// The body names exactly `provider_key` and `subject`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_body_member_outside_the_schema_is_malformed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, user) = f.user().await;
    let body = json!({ "provider_key": "example-oidc", "subject": "1", "user_id": user });
    let answer = f
        .send(Method::POST, &user_identities(user), &admin, Some(body))
        .await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "request.malformed")
    );
}

// Listing (B23, T18).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admin_reads_a_users_identities_with_their_attributes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (viewer, _) = f.holder(&["user.view"]).await;
    let (_, user) = f.user().await;
    let attributes = json!({ "rating": 5, "subdivision": "EXA" });
    let first = f
        .link(f.a.id, user, f.a.oidc, "1000001", attributes.clone())
        .await;
    let second = f.link(f.a.id, user, f.a.oauth, "sub-2", json!({})).await;
    let (_, other) = f.user().await;
    f.link(f.a.id, other, f.a.oidc, "1000002", json!({})).await;

    let answer = f
        .send(Method::GET, &user_identities(user), &viewer, None)
        .await;

    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    let items = answer.body.as_array().expect("an array");
    let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [first.to_string(), second.to_string()]);
    assert_eq!(items[0]["attributes"], attributes);
    assert_eq!(items[1]["provider_key"], "example-oauth");
    let document = serde_json::to_value(openapi::document()).unwrap();
    for item in items {
        let mut item = item.clone();
        // The checker reads no free-form object; `attributes` is checked above.
        item["attributes"] = json!({});
        let errors = common::schema_errors(&document, "Identity", &item);
        assert!(errors.is_empty(), "{errors:#?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t18_my_identities_carry_the_label_and_never_the_attributes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, me) = f.user().await;
    let secret = json!({ "email_hint": "private-value" });
    let labeled = f
        .link(f.a.id, me, f.a.oidc, "1000001", secret.clone())
        .await;
    let unlabeled = f.link(f.a.id, me, f.a.oauth, "sub-2", secret).await;
    let (_, other) = f.user().await;
    f.link(f.a.id, other, f.a.oidc, "1000002", json!({})).await;

    let answer = f.send(Method::GET, MY_IDENTITIES, &cookie, None).await;

    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
    assert!(
        !answer.body.to_string().contains("private-value"),
        "{answer:?}"
    );
    let items = answer.body.as_array().expect("an array");
    assert_eq!(
        items[0],
        json!({
            "id": labeled,
            "provider_key": "example-oidc",
            "provider_label": "Sign in with Example",
            "subject": "1000001",
            "created_at": "2025-11-13T02:13:20.000000Z",
        })
    );
    assert_eq!(items[1]["id"], unlabeled.to_string());
    assert_eq!(items[1]["provider_label"], Value::Null);
    assert_eq!(items.len(), 2);
    let document = serde_json::to_value(openapi::document()).unwrap();
    for item in items {
        let errors = common::schema_errors(&document, "MyIdentity", item);
        assert!(errors.is_empty(), "{errors:#?}");
    }
}

// Removing (B19, T16).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_an_admin_cannot_remove_a_users_last_identity(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, user) = f.user().await;
    let only = f.link(f.a.id, user, f.a.oidc, "1000001", json!({})).await;
    let before = f.table().await;

    let refused = f.delete(&admin, user, only).await;

    assert_eq!(
        (refused.status, refused.code()),
        (StatusCode::CONFLICT, "identity.last_sign_in_method")
    );
    assert_eq!(f.table().await, before);
    f.link(f.a.id, user, f.a.oauth, "sub-2", json!({})).await;
    let removed = f.delete(&admin, user, only).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    assert_eq!(removed.body, Value::Null);
    let left = identities::find_user(&f.pool, scope(f.a.id), f.a.oidc, "1000001").await;
    assert_eq!(left.unwrap(), None);
}

/// A local identity is removed through neither route, even with other identities left: a local
/// account changes through spec 04's routes (spec 04 B3).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn neither_route_removes_a_local_identity(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let local = identity_providers::get_by_key(&f.pool, scope(f.a.id), "local")
        .await
        .unwrap()
        .expect("the local row")
        .id;
    let (cookie, me) = f.user().await;
    let mine = f.link(f.a.id, me, local, &me.to_string(), json!({})).await;
    f.link(f.a.id, me, f.a.oidc, "1000001", json!({})).await;
    f.link(f.a.id, me, f.a.oauth, "sub-2", json!({})).await;
    let before = f.table().await;

    let by_admin = f.delete(&admin, me, mine).await;
    let by_me = f.delete_mine(&cookie, mine).await;

    for refused in [by_admin, by_me] {
        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::CONFLICT, "identity.local"),
            "{refused:?}"
        );
    }
    assert_eq!(f.table().await, before);
}

/// An admin can remove an identity of a user an outside source owns (B19).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admin_removes_an_identity_of_an_externally_managed_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (_, user) = f.user_in(f.a.id, Source::Idp).await;
    let first = f.link(f.a.id, user, f.a.oidc, "1000001", json!({})).await;
    f.link(f.a.id, user, f.a.oauth, "sub-2", json!({})).await;

    let removed = f.delete(&admin, user, first).await;

    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_user_removes_their_own_identity_but_not_the_last(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (cookie, me) = f.user().await;
    let first = f.link(f.a.id, me, f.a.oidc, "1000001", json!({})).await;
    let second = f.link(f.a.id, me, f.a.oauth, "sub-2", json!({})).await;

    let removed = f.delete_mine(&cookie, first).await;
    let last = f.delete_mine(&cookie, second).await;

    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    assert_eq!(
        (last.status, last.code()),
        (StatusCode::CONFLICT, "identity.last_sign_in_method")
    );
    let left = identities::list_for_user(&f.pool, scope(f.a.id), me)
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, second);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_user_from_an_outside_source_cannot_remove_their_identities(pool: PgPool) {
    let f = Fixture::new(pool).await;
    for source in [Source::Idp, Source::Sync, Source::Scim, Source::Connector] {
        let (cookie, me) = f.user_in(f.a.id, source).await;
        let subject = format!("{}-1", source.as_str());
        let first = f.link(f.a.id, me, f.a.oidc, &subject, json!({})).await;
        let subject = format!("{}-2", source.as_str());
        f.link(f.a.id, me, f.a.oidc, &subject, json!({})).await;
        let before = f.table().await;

        let refused = f.delete_mine(&cookie, first).await;

        assert_eq!(
            (refused.status, refused.code()),
            (StatusCode::CONFLICT, "identity.externally_managed"),
            "{source:?}"
        );
        assert_eq!(f.table().await, before);
    }
}

// Other users' and organizations' ids (T20).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t20_ids_of_another_user_or_organization_are_not_found(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (cookie, me) = f.user().await;
    f.link(f.a.id, me, f.a.oidc, "mine-1", json!({})).await;
    f.link(f.a.id, me, f.a.oidc, "mine-2", json!({})).await;
    let (_, sam) = f.user().await;
    let sams = f.link(f.a.id, sam, f.a.oidc, "sam-1", json!({})).await;
    f.link(f.a.id, sam, f.a.oidc, "sam-2", json!({})).await;
    let (_, b_user) = f.user_in(f.b.id, Source::Local).await;
    let theirs = f.link(f.b.id, b_user, f.b.oidc, "b-1", json!({})).await;
    f.link(f.b.id, b_user, f.b.oidc, "b-2", json!({})).await;
    let before = f.table().await;

    let not_found = |answer: Answer, code: &str| {
        assert_eq!(
            (answer.status, answer.code()),
            (StatusCode::NOT_FOUND, code)
        );
    };
    let b_path = user_identities(b_user);
    not_found(
        f.send(Method::GET, &b_path, &admin, None).await,
        "user.not_found",
    );
    not_found(
        f.post(&admin, b_user, "example-oidc", "new").await,
        "user.not_found",
    );
    not_found(f.delete(&admin, b_user, theirs).await, "user.not_found");
    // Org A's user with org B's identity, or with another user's.
    not_found(f.delete(&admin, me, theirs).await, "identity.not_found");
    not_found(f.delete(&admin, me, sams).await, "identity.not_found");
    not_found(f.delete_mine(&cookie, sams).await, "identity.not_found");
    not_found(f.delete_mine(&cookie, theirs).await, "identity.not_found");
    assert_eq!(f.table().await, before);
}

// The escalation rule (coordinator decision, 14 Nov 2026).

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_admin_cannot_change_how_a_more_privileged_user_signs_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (manager, manager_id) = f.holder(&["user.manage", "user.view"]).await;
    let (_, org_admin) = f.admin().await;
    let (_, tower_admin) = f.user().await;
    let account_admin = f.builtin("account_admin").await;
    f.assign(tower_admin, account_admin, Scope::Account(f.a.tower))
        .await;
    for target in [org_admin, tower_admin] {
        let first = f
            .link(f.a.id, target, f.a.oidc, &format!("{target}-1"), json!({}))
            .await;
        f.link(f.a.id, target, f.a.oidc, &format!("{target}-2"), json!({}))
            .await;
        let before = f.table().await;

        let linked = f.post(&manager, target, "example-oauth", "attacker").await;
        let removed = f.delete(&manager, target, first).await;

        for answer in [linked, removed] {
            assert_eq!(
                (answer.status, answer.code()),
                (StatusCode::FORBIDDEN, "permission.escalation"),
                "{answer:?}"
            );
        }
        assert_eq!(f.table().await, before);
    }
    // A user with no role, and the caller themselves, pass.
    let (_, plain) = f.user().await;
    for (target, subject) in [(plain, "plain"), (manager_id, "manager")] {
        let linked = f.post(&manager, target, "example-oauth", subject).await;
        assert_eq!(linked.status, StatusCode::CREATED, "{linked:?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_org_admin_may_change_how_another_org_admin_signs_in(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, me) = f.admin().await;
    let (_, other) = f.admin().await;
    for target in [other, me] {
        let linked = f
            .post(&admin, target, "example-oidc", &target.to_string())
            .await;
        assert_eq!(linked.status, StatusCode::CREATED, "{linked:?}");
    }
}

/// The admin list is a read, so the escalation rule does not apply to it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn reading_a_more_privileged_users_identities_is_allowed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (viewer, _) = f.holder(&["user.view"]).await;
    let (_, org_admin) = f.admin().await;
    let answer = f
        .send(Method::GET, &user_identities(org_admin), &viewer, None)
        .await;
    assert_eq!(answer.status, StatusCode::OK, "{answer:?}");
}

// Audit entries (spec 09 B22, coordinator decision of 2026-11-14).

impl Fixture {
    /// Org A's `identity` audit entries, in write order.
    async fn identity_entries(&self) -> Vec<audit::Entry> {
        audit::list(&self.pool, scope(self.a.id), 1000)
            .await
            .expect("read entries")
            .into_iter()
            .filter(|e| e.target_type == "identity")
            .collect()
    }
}

/// The caller of a request as its audit entry records it.
fn actor(user: Uuid) -> Actor {
    Actor::User {
        id: user,
        ip: CLIENT.ip(),
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn each_identity_write_leaves_one_audit_entry(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f.admin().await;
    let (cookie, me) = f.user().await;
    let mine = f.link(f.a.id, me, f.a.oidc, "mine-1", json!({})).await;
    f.link(f.a.id, me, f.a.oidc, "mine-2", json!({})).await;
    let (_, user) = f.user().await;
    let base = f.identity_entries().await.len();

    let linked = f.post(&admin, user, "example-oauth", "1000001").await;
    assert_eq!(linked.status, StatusCode::CREATED, "{linked:?}");
    let linked_id: Uuid = linked.body["id"].as_str().unwrap().parse().unwrap();
    f.link(f.a.id, user, f.a.oidc, "1000002", json!({})).await;
    let base_after_fixture = f.identity_entries().await.len();
    let removed = f.delete(&admin, user, linked_id).await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{removed:?}");
    let mine_removed = f.delete_mine(&cookie, mine).await;
    assert_eq!(
        mine_removed.status,
        StatusCode::NO_CONTENT,
        "{mine_removed:?}"
    );

    let entries = f.identity_entries().await;
    assert_eq!(base_after_fixture, base + 2);
    assert_eq!(entries.len(), base + 4);
    let identity = |user: Uuid, key: &str, subject: &str| json!({ "user_id": user, "provider_key": key, "subject": subject });
    let seen: Vec<_> = [&entries[base], &entries[base + 2], &entries[base + 3]]
        .iter()
        .map(|e| (e.actor, e.action.clone(), e.target_id, e.data.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            (
                actor(admin_id),
                "identity.link".to_owned(),
                linked_id,
                json!({ "after": identity(user, "example-oauth", "1000001") })
            ),
            (
                actor(admin_id),
                "identity.unlink".to_owned(),
                linked_id,
                json!({ "before": identity(user, "example-oauth", "1000001") })
            ),
            (
                actor(me),
                "identity.unlink".to_owned(),
                mine,
                json!({ "before": identity(me, "example-oidc", "mine-1") })
            ),
        ]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_identity_write_leaves_no_audit_entry(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, _) = f.admin().await;
    let (manager, _) = f.holder(&["user.manage", "user.view"]).await;
    let (_, org_admin) = f.admin().await;
    let (cookie, me) = f.user().await;
    let only = f.link(f.a.id, me, f.a.oidc, "mine-1", json!({})).await;
    let (idp_cookie, _) = f.user_in(f.a.id, Source::Idp).await;
    let before = f.identity_entries().await;

    let refusals = [
        f.post(&admin, me, "example-oidc", "mine-1").await, // taken
        f.post(&admin, me, "missing", "x").await,           // unknown provider
        f.post(&manager, org_admin, "example-oidc", "x").await, // escalation
        f.delete(&admin, me, only).await,                   // last sign-in method
        f.delete_mine(&cookie, only).await,                 // last sign-in method
        f.delete_mine(&idp_cookie, only).await,             // externally managed
    ];

    for answer in &refusals {
        assert!(answer.status.is_client_error(), "{answer:?}");
    }
    assert_eq!(f.identity_entries().await, before);
}

/// Identities linked while creating a user write their entries too.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn identities_linked_at_user_creation_are_audited(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let (admin, admin_id) = f.admin().await;
    let body = json!({
        "display_name": "Casey Example",
        "identities": [{ "provider_key": "example-oidc", "subject": "1000009" }],
    });
    let created = f
        .send(Method::POST, "/api/v1/users", &admin, Some(body))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{created:?}");
    let entries = f.identity_entries().await;
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].actor, actor(admin_id));
    assert_eq!(entries[0].action, "identity.link");
    assert_eq!(entries[0].data["after"]["subject"], "1000009");
}

/// The claim mapping refuses a subject longer than the `identities` table takes, so the two
/// limits must be one number: a longer auth limit would fail at the insert, a shorter one would
/// refuse subjects the table could store.
#[test]
fn the_mapping_subject_limit_is_the_identities_limit() {
    assert_eq!(
        stepclimb_auth::claims::MAX_SUBJECT_CHARS,
        stepclimb_core::identities::MAX_SUBJECT_CHARS
    );
}
