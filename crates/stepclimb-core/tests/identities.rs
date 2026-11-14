// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identity providers and identities repositories and their tables against a fresh database
//! per test (docs/specs/m0/03-users-and-identities.md B13 to B16, T11 to T13). Needs
//! `DATABASE_URL` (`just up`).
//!
//! Each test builds two organizations, A and B, each with two users and two providers (an OIDC
//! one and an OAuth 2 one), and checks that org A's scope reads and changes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    id::{Uuid, new_id},
    identities::{self, Error, Identity, NewIdentity},
    identity_providers::{self, Kind},
    organizations,
    users::{self, NewUser, Status},
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn later() -> SystemTime {
    request_time() + Duration::from_secs(3_600)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

struct Org {
    id: Uuid,
    /// A user with an email address.
    sam: Uuid,
    pat: Uuid,
    /// An OIDC provider, key `oidc-example`.
    oidc: Uuid,
    /// An OAuth 2 provider, key `oauth-example`.
    oauth: Uuid,
}

async fn user(pool: &PgPool, org: Uuid, name: &str, email: Option<&str>) -> Uuid {
    let user = NewUser {
        display_name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: email.map(Into::into),
        locale: None,
        tz: "UTC".into(),
        source: Source::Idp,
        external_id: None,
    };
    users::create(pool, scope(org), &user, request_time())
        .await
        .expect("create user")
        .id
}

/// Insert a provider row: the write functions belong to specs 05 and 06.
async fn provider(pool: &PgPool, org: Uuid, key: &str, kind: &str, enabled: bool) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, $3, $4, $5::jsonb, $6)",
    )
    .bind(id)
    .bind(org)
    .bind(key)
    .bind(kind)
    .bind(r#"{"label": "Example", "client_secret": "sealed"}"#)
    .bind(enabled)
    .execute(pool)
    .await
    .expect("insert provider");
    id
}

async fn org(pool: &PgPool, slug: &str) -> Org {
    let id = organizations::create(pool, slug, slug, request_time())
        .await
        .expect("create organization")
        .id;
    Org {
        id,
        sam: user(pool, id, "Sam Ortiz", Some("sam@example.test")).await,
        pat: user(pool, id, "Pat Example", None).await,
        oidc: provider(pool, id, "oidc-example", "oidc", true).await,
        oauth: provider(pool, id, "oauth-example", "oauth2", true).await,
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn new(user_id: Uuid, provider_id: Uuid, subject: &str) -> NewIdentity {
    NewIdentity {
        user_id,
        provider_id,
        subject: subject.into(),
        attributes: Map::new(),
    }
}

async fn link(pool: &PgPool, org: Uuid, identity: NewIdentity) -> Identity {
    identities::link(pool, scope(org), &identity, request_time())
        .await
        .expect("link identity")
}

/// Every `identities` row as text, in id order.
async fn table(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT to_jsonb(i)::text FROM identities i ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("read identities")
}

/// Assert that `result` is an `Err` matching `pattern`.
macro_rules! assert_err {
    ($result:expr, $pattern:pat $(,)?) => {{
        let result = $result;
        assert!(
            matches!(result, Err($pattern)),
            "expected Err({}), got {result:?}",
            stringify!($pattern)
        );
    }};
}

async fn try_link(pool: &PgPool, org: Uuid, identity: NewIdentity) -> Result<Identity, Error> {
    identities::link(pool, scope(org), &identity, request_time()).await
}

/// The SQLSTATE and constraint of a failed statement.
fn violation(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> (String, String) {
    let error = result.expect_err("the database refuses the row");
    let db = error.as_database_error().expect("a database error");
    (
        db.code().unwrap_or_default().into_owned(),
        db.constraint().unwrap_or_default().to_owned(),
    )
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn link_stores_the_identity_and_reads_it_back_with_its_provider_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let attributes = object(json!({ "rating": 5, "subdivision": "EXA" }));

    let linked = identities::link(
        &pool,
        scope(a.id),
        &NewIdentity {
            attributes: attributes.clone(),
            ..new(a.sam, a.oauth, "1000001")
        },
        request_time(),
    )
    .await
    .unwrap();

    assert_eq!(linked.user_id, a.sam);
    assert_eq!(linked.provider_id, a.oauth);
    assert_eq!(linked.provider_key, "oauth-example");
    assert_eq!(linked.subject, "1000001");
    assert_eq!(linked.attributes, attributes);
    assert_eq!(linked.created_at, request_time());
    assert_eq!(linked.updated_at, request_time());
    let org_id: Uuid = sqlx::query_scalar("SELECT org_id FROM identities WHERE id = $1")
        .bind(linked.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(org_id, a.id);
    let listed = identities::list_for_user(&pool, scope(a.id), a.sam).await;
    assert_eq!(listed.unwrap(), [linked]);
}

/// T11: `(provider, subject)` maps to at most one user; the same subject under another provider
/// is another identity, and one user may hold several identities, even at one provider (B14).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_provider_subject_links_to_at_most_one_user(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    let before = table(&pool).await;

    let taken = try_link(&pool, a.id, new(a.pat, a.oauth, "1000001")).await;
    let again = try_link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    assert_err!(taken, Error::Taken);
    assert_err!(again, Error::Taken);
    assert_eq!(table(&pool).await, before);

    link(&pool, a.id, new(a.pat, a.oidc, "1000001")).await;
    link(&pool, a.id, new(a.sam, a.oauth, "1000002")).await;
    link(&pool, b.id, new(b.sam, b.oauth, "1000001")).await;
    let sams = identities::list_for_user(&pool, scope(a.id), a.sam)
        .await
        .unwrap();
    let subjects: Vec<&str> = sams.iter().map(|i| i.subject.as_str()).collect();
    assert_eq!(subjects, ["1000001", "1000002"]);
}

/// The longest subject, in four-byte characters, still fits the `(provider_id, subject)` unique
/// index.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_longest_subject_fits_the_unique_index(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let widest = "\u{1F600}".repeat(identities::MAX_SUBJECT_CHARS);
    let linked = link(&pool, a.id, new(a.sam, a.oauth, &widest)).await;
    assert_eq!(linked.subject, widest);
}

/// T12 through the repository: a user or provider outside the scope's organization, or one that
/// does not exist, is refused, as are an empty or too long subject and NUL, and nothing is inserted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn link_refuses_what_is_outside_the_scope_and_inserts_nothing(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    link(&pool, b.id, new(b.sam, b.oauth, "2000001")).await;
    let before = table(&pool).await;
    let nul_attributes = NewIdentity {
        attributes: object(json!({ "team": "a\u{0}" })),
        ..new(a.sam, a.oauth, "1000003")
    };
    let link = |identity| try_link(&pool, a.id, identity);

    assert_err!(
        link(new(b.sam, a.oauth, "1000002")).await,
        Error::UserNotFound
    );
    assert_err!(
        link(new(new_id(), a.oauth, "1000002")).await,
        Error::UserNotFound
    );
    assert_err!(
        link(new(a.sam, b.oauth, "1000002")).await,
        Error::ProviderNotFound
    );
    // A subject linked at org B's provider answers the same as a free one: org A learns nothing.
    assert_err!(
        link(new(a.sam, b.oauth, "2000001")).await,
        Error::ProviderNotFound
    );
    assert_err!(
        link(new(b.sam, b.oauth, "2000001")).await,
        Error::ProviderNotFound
    );
    assert_err!(
        link(new(a.sam, new_id(), "1000002")).await,
        Error::ProviderNotFound
    );
    assert_err!(link(new(a.sam, a.oauth, "")).await, Error::SubjectEmpty);
    let over = "1".repeat(identities::MAX_SUBJECT_CHARS + 1);
    assert_err!(
        link(new(a.sam, a.oauth, &over)).await,
        Error::SubjectTooLong
    );
    assert_err!(
        link(new(a.sam, a.oauth, "10\0")).await,
        Error::ContainsNul { field: "subject" }
    );
    assert_err!(
        link(nul_attributes).await,
        Error::ContainsNul {
            field: "attributes"
        }
    );

    assert_eq!(table(&pool).await, before);
}

/// T12 at the database: an identity whose `org_id` differs from its user's or its provider's
/// organization is refused by the composite foreign keys.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_foreign_keys_keep_an_identity_in_one_organization(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let insert = |org: Uuid, user: Uuid, provider: Uuid| {
        sqlx::query(
            "INSERT INTO identities (id, org_id, user_id, provider_id, subject)
             VALUES ($1, $2, $3, $4, '1000001')",
        )
        .bind(new_id())
        .bind(org)
        .bind(user)
        .bind(provider)
        .execute(&pool)
    };

    let foreign_user = violation(insert(a.id, b.sam, a.oauth).await);
    let foreign_provider = violation(insert(a.id, a.sam, b.oauth).await);
    let both_foreign = violation(insert(a.id, b.sam, b.oauth).await);

    let fkey = "23503".to_owned();
    assert_eq!(
        foreign_user,
        (fkey.clone(), "identities_org_id_user_id_fkey".into())
    );
    assert_eq!(
        foreign_provider,
        (fkey.clone(), "identities_org_id_provider_id_fkey".into())
    );
    assert_eq!(both_foreign.0, fkey);
    assert!(table(&pool).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_tables_refuse_values_outside_their_checks(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let insert_provider = |key: &'static str, kind: &'static str, config: &'static str| {
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config)
             VALUES ($1, $2, $3, $4, $5::jsonb)",
        )
        .bind(new_id())
        .bind(a.id)
        .bind(key)
        .bind(kind)
        .bind(config)
        .execute(&pool)
    };
    let insert_identity = |subject: &'static str, attributes: &'static str| {
        sqlx::query(
            "INSERT INTO identities (id, org_id, user_id, provider_id, subject, attributes)
             VALUES ($1, $2, $3, $4, $5, $6::jsonb)",
        )
        .bind(new_id())
        .bind(a.id)
        .bind(a.sam)
        .bind(a.oauth)
        .bind(subject)
        .bind(attributes)
        .execute(&pool)
    };

    let saml = violation(insert_provider("saml", "saml", "{}").await);
    let array_config = violation(insert_provider("other", "oidc", "[]").await);
    let same_key = violation(insert_provider("oidc-example", "oidc", "{}").await);
    let empty_subject = violation(insert_identity("", "{}").await);
    let array_attributes = violation(insert_identity("1000001", "[1]").await);

    assert_eq!(
        saml,
        ("23514".into(), "identity_providers_kind_check".into())
    );
    assert_eq!(
        array_config,
        ("23514".into(), "identity_providers_config_check".into())
    );
    assert_eq!(
        same_key,
        ("23505".into(), "identity_providers_org_id_key_key".into())
    );
    assert_eq!(
        empty_subject,
        ("23514".into(), "identities_subject_check".into())
    );
    assert_eq!(
        array_attributes,
        ("23514".into(), "identities_attributes_check".into())
    );
    let b = org(&pool, "org-b").await;
    let reused = identity_providers::get_by_key(&pool, scope(b.id), "oidc-example").await;
    assert_eq!(
        reused.unwrap().map(|p| p.id),
        Some(b.oidc),
        "another organization may reuse a key"
    );
}

/// T13 (B16, B17): the linked user and status for a known `(provider, subject)`, none for
/// anything else, and never a match by email.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn find_user_returns_the_linked_user_for_a_known_identity_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let identity = link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    link(&pool, b.id, new(b.pat, b.oauth, "1000002")).await;
    let find = |org: Uuid, provider: Uuid, subject: &'static str| {
        let pool = pool.clone();
        async move { identities::find_user(&pool, scope(org), provider, subject).await }
    };

    let found = find(a.id, a.oauth, "1000001").await.unwrap().unwrap();
    let sam = users::get(&pool, scope(a.id), a.sam)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.identity_id, identity.id);
    assert_eq!(found.user, sam);
    assert_eq!(found.user.status, Status::Active);

    users::set_status(
        &pool,
        scope(a.id),
        a.sam,
        Status::Suspended,
        later(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let suspended = find(a.id, a.oauth, "1000001").await.unwrap().unwrap();
    assert_eq!(suspended.user.status, Status::Suspended);

    let misses = [
        (a.id, a.oauth, "1000009"),
        (a.id, a.oidc, "1000001"),
        (a.id, a.oauth, "1000001 "),
        (a.id, b.oauth, "1000002"),
        (b.id, a.oauth, "1000001"),
        (a.id, a.oauth, "sam@example.test"),
        (a.id, a.oidc, "sam@example.test"),
        (a.id, a.oauth, ""),
    ];
    for (org, provider, subject) in misses {
        let result = find(org, provider, subject).await.unwrap();
        assert_eq!(result, None, "{subject:?}");
    }
    let nul = find(a.id, a.oauth, "10\0").await;
    assert_err!(nul, Error::ContainsNul { field: "subject" });
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_for_user_reads_one_users_identities_in_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let first = link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    let second = link(&pool, a.id, new(a.sam, a.oidc, "sub-1")).await;
    link(&pool, a.id, new(a.pat, a.oauth, "1000002")).await;
    link(&pool, b.id, new(b.sam, b.oauth, "1000001")).await;

    let sams = identities::list_for_user(&pool, scope(a.id), a.sam)
        .await
        .unwrap();
    let across = identities::list_for_user(&pool, scope(a.id), b.sam)
        .await
        .unwrap();
    let unknown = identities::list_for_user(&pool, scope(a.id), new_id())
        .await
        .unwrap();

    assert_eq!(sams, [first, second]);
    assert_eq!(sams[1].provider_key, "oidc-example");
    assert!(across.is_empty());
    assert!(unknown.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unlink_deletes_an_identity_of_that_user_in_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let mine = link(&pool, a.id, new(a.sam, a.oauth, "1000001")).await;
    let pats = link(&pool, a.id, new(a.pat, a.oauth, "1000002")).await;
    let theirs = link(&pool, b.id, new(b.sam, b.oauth, "1000003")).await;
    let before = table(&pool).await;

    let other_user = identities::unlink(&pool, scope(a.id), a.sam, pats.id).await;
    let other_org = identities::unlink(&pool, scope(a.id), b.sam, theirs.id).await;
    let other_org_own_user = identities::unlink(&pool, scope(a.id), a.sam, theirs.id).await;
    assert_err!(other_user, Error::NotFound);
    assert_err!(other_org, Error::NotFound);
    assert_err!(other_org_own_user, Error::NotFound);
    assert_eq!(table(&pool).await, before);

    identities::unlink(&pool, scope(a.id), a.sam, mine.id)
        .await
        .unwrap();
    let again = identities::unlink(&pool, scope(a.id), a.sam, mine.id).await;
    assert_err!(again, Error::NotFound);
    let found = identities::find_user(&pool, scope(a.id), a.oauth, "1000001").await;
    assert_eq!(found.unwrap(), None);
    assert_eq!(table(&pool).await.len(), 2);
}

/// B15: the attributes are replaced, not merged, and `updated_at` moves.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_attributes_replaces_them_in_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let mine = link(
        &pool,
        a.id,
        NewIdentity {
            attributes: object(json!({ "rating": 3, "team": "a" })),
            ..new(a.sam, a.oauth, "1000001")
        },
    )
    .await;
    let theirs = link(&pool, b.id, new(b.sam, b.oauth, "1000001")).await;
    let replacement = object(json!({ "rating": 5 }));

    let updated = identities::update_attributes(&pool, scope(a.id), mine.id, &replacement, later())
        .await
        .unwrap();

    assert_eq!(updated.attributes, replacement);
    assert_eq!(updated.created_at, request_time());
    assert_eq!(updated.updated_at, later());
    assert_eq!(updated.provider_key, "oauth-example");
    let before = table(&pool).await;
    let across =
        identities::update_attributes(&pool, scope(a.id), theirs.id, &replacement, later()).await;
    let unknown =
        identities::update_attributes(&pool, scope(a.id), new_id(), &replacement, later()).await;
    let nul = identities::update_attributes(
        &pool,
        scope(a.id),
        mine.id,
        &object(json!({ "t": ["\u{0}"] })),
        later(),
    )
    .await;
    assert_err!(across, Error::NotFound);
    assert_err!(unknown, Error::NotFound);
    assert_err!(
        nul,
        Error::ContainsNul {
            field: "attributes"
        }
    );
    assert_eq!(table(&pool).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_by_key_finds_a_provider_of_the_scope_only(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    let only_b = provider(&pool, b.id, "only-b", "oidc", true).await;
    let disabled = provider(&pool, a.id, "disabled", "oidc", false).await;

    let oauth = identity_providers::get_by_key(&pool, scope(a.id), "oauth-example")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(oauth.id, a.oauth);
    assert_eq!(oauth.kind, Kind::Oauth2);
    assert!(oauth.enabled);
    assert_eq!(
        Value::Object(oauth.config),
        json!({ "label": "Example", "client_secret": "sealed" })
    );
    let off = identity_providers::get_by_key(&pool, scope(a.id), "disabled")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((off.id, off.enabled), (disabled, false));

    for key in ["only-b", "missing", "OAUTH-EXAMPLE", ""] {
        let found = identity_providers::get_by_key(&pool, scope(a.id), key).await;
        assert!(found.unwrap().is_none(), "{key:?}");
    }
    let in_b = identity_providers::get_by_key(&pool, scope(b.id), "only-b").await;
    assert_eq!(in_b.unwrap().unwrap().id, only_b);
    let nul = identity_providers::get_by_key(&pool, scope(a.id), "oidc\0").await;
    assert!(matches!(
        nul,
        Err(identity_providers::Error::ContainsNul { field: "key" })
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_enabled_lists_the_scopes_enabled_providers_by_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;
    provider(&pool, a.id, "disabled", "oidc", false).await;
    let zeta = provider(&pool, a.id, "zeta", "oauth2", true).await;
    provider(&pool, b.id, "alpha", "oidc", true).await;

    let listed = identity_providers::list_enabled(&pool, scope(a.id))
        .await
        .unwrap();

    let ids: Vec<Uuid> = listed.iter().map(|p| p.id).collect();
    assert_eq!(ids, [a.oauth, a.oidc, zeta]);
    assert!(listed.iter().all(|p| p.enabled));
    assert_eq!(listed[1].kind, Kind::Oidc);
    assert_eq!(listed[1].created_at, listed[1].updated_at);
}
