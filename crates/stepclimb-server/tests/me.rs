// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /api/v1/me` over the whole router (docs/specs/m0/03-users-and-identities.md B21, B22,
//! T6, T17). The 401, 2xx and cross-organization cases of the permission suite are in
//! `tests/permissions.rs` (its `getMe` entry).
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
    OrgScope, Source,
    clock::ManualClock,
    id::Uuid,
    organizations,
    users::{self, NewUser, Status},
};
use stepclimb_server::{AppState, openapi, router, session, web_origin::WebOrigin};
use tower::ServiceExt;

const ME: &str = "/api/v1/me";

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
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
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
        users::set_status(&self.pool, scope(self.org), user, status, now())
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
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the body is readable")
            .to_bytes();
        let body = serde_json::from_slice(&bytes).expect("a JSON body");
        Answer { status, body }
    }

    async fn me_with_cookie(&self, cookie: &str) -> Answer {
        self.get_me(&[(header::COOKIE, cookie)]).await
    }
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
            "features": [],
        })
    );
    let document = serde_json::to_value(openapi::document()).expect("the document as JSON");
    let schema = schema_ref(&document, "#/components/schemas/Me");
    let mut errors = Vec::new();
    conforms(&document, schema, &answer.body, "", &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");
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
    let document = serde_json::to_value(openapi::document()).expect("the document as JSON");
    let mut errors = Vec::new();
    let schema = schema_ref(&document, "#/components/schemas/Me");
    conforms(&document, schema, &answer.body, "", &mut errors);
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

/// No bearer credential is accepted yet (API keys are spec 08 issue 6), and a cookie beside a
/// bearer header never authenticates the request (spec 08 B16).
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
    assert_eq!(answer.code(), "auth.unauthenticated");
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

// A check of a value against the subset of JSON Schema utoipa writes for these types: `$ref`,
// `type` (one name or a list), `enum`, `properties` with `required`, `items` and `format: uuid`.
// A member the schema does not declare is an error too, so the answer cannot carry more than the
// document says.

fn schema_ref<'a>(document: &'a Value, reference: &str) -> &'a Value {
    let pointer = reference.strip_prefix('#').expect("a local reference");
    document
        .pointer(pointer)
        .unwrap_or_else(|| panic!("{reference} is not in the document"))
}

fn type_matches(name: &str, value: &Value) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn conforms(document: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return conforms(document, schema_ref(document, reference), value, at, errors);
    }
    if let Some(types) = schema.get("type") {
        let names: Vec<&str> = match types {
            Value::String(name) => vec![name.as_str()],
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !names.iter().any(|name| type_matches(name, value)) {
            errors.push(format!("{at}: {value} is not of type {types}"));
            return;
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        errors.push(format!("{at}: {value} is not one of {allowed:?}"));
    }
    if schema.get("format").and_then(Value::as_str) == Some("uuid")
        && let Some(text) = value.as_str()
        && Uuid::try_parse(text).is_err()
    {
        errors.push(format!("{at}: {text} is not a UUID"));
    }
    if let (Some(object), Some(properties)) = (
        value.as_object(),
        schema.get("properties").and_then(Value::as_object),
    ) {
        let required = schema.get("required").and_then(Value::as_array);
        for name in required.into_iter().flatten().filter_map(Value::as_str) {
            if !object.contains_key(name) {
                errors.push(format!("{at}/{name}: required but missing"));
            }
        }
        for (name, member) in object {
            match properties.get(name) {
                Some(property) => {
                    conforms(document, property, member, &format!("{at}/{name}"), errors);
                }
                None => errors.push(format!("{at}/{name}: not declared in the schema")),
            }
        }
    }
    if let (Some(items), Some(schema_items)) = (value.as_array(), schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            conforms(
                document,
                schema_items,
                item,
                &format!("{at}/{index}"),
                errors,
            );
        }
    }
}

/// The validator above must be able to fail: a missing member, an undeclared one, a wrong type
/// and a value outside an enum are each reported against the real `Me` schema.
#[test]
fn the_schema_check_refuses_answers_the_document_does_not_describe() {
    let document = serde_json::to_value(openapi::document()).expect("the document as JSON");
    let schema = schema_ref(&document, "#/components/schemas/Me");
    let good = json!({
        "user": {
            "id": "0190a000-0000-7000-8000-000000000000", "display_name": "Sam Ortiz",
            "given_name": "Sam", "family_name": "Ortiz", "email": null, "locale": "en",
            "tz": "UTC", "status": "active", "source": "local",
        },
        "organization": {
            "id": "0190a000-0000-7000-8000-000000000001", "slug": "example", "name": "Example Org",
        },
        "grants": [{
            "scope": { "type": "account", "id": "0190a000-0000-7000-8000-000000000002" },
            "permissions": ["account.view"],
        }],
        "features": ["custom_roles"],
    });
    let mut errors = Vec::new();
    conforms(&document, schema, &good, "", &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");

    let broken = [
        ("/user/tz", Value::Null),
        ("/user/status", json!("deleted")),
        ("/grants/0/scope/type", json!("planet")),
        ("/organization/id", json!("not-a-uuid")),
        ("/features", json!("custom_roles")),
    ];
    for (pointer, replacement) in broken {
        let mut value = good.clone();
        *value.pointer_mut(pointer).expect("a member") = replacement;
        let mut errors = Vec::new();
        conforms(&document, schema, &value, "", &mut errors);
        assert!(!errors.is_empty(), "{pointer} was accepted");
    }
    let mut extra = good.clone();
    extra["user"]["external_id"] = json!("E1");
    let mut missing = good.clone();
    missing["user"]
        .as_object_mut()
        .expect("an object")
        .remove("email");
    for value in [extra, missing] {
        let mut errors = Vec::new();
        conforms(&document, schema, &value, "", &mut errors);
        assert!(!errors.is_empty(), "{value} was accepted");
    }
}
