// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission pipeline against the database (docs/specs/m0/07-admission.md B1 to B4, B7,
//! B20, B21, Q6; T1, T6, T15 to T17). The tests call `admission::admit` and
//! `admission::complete` as a provider's callback does, on a transaction of their own, with an
//! OIDC provider row inserted by SQL; the passkey routes' calls are covered by
//! `passkey_enrollment.rs` and `passkey_sign_in.rs`. All data is synthetic.
//!
//! Needs `DATABASE_URL` (`just up`).

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, StatusCode, header};
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider},
    organizations,
    users::{self, NewUser, Status, User},
};
use stepclimb_server::providers::Providers;
use stepclimb_server::{
    admission::{self, Admission, Completion, Context, Profile, Step, Verified},
    audit::ClientAddress,
};

/// The client address of every sign-in (TEST-NET-1).
const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

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
    reason = "tests stand in for the routes that resolve the request's organization"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn attributes(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("attributes are an object, not {other}"),
    }
}

fn profile(given: Option<&str>, family: Option<&str>, email: Option<&str>) -> Profile {
    Profile {
        given_name: given.map(str::to_owned),
        family_name: family.map(str::to_owned),
        email: email.map(str::to_owned),
    }
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    provider: IdentityProvider,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = Self::org(&pool, "org-a").await;
        let provider = Self::provider_in(&pool, org, "sso").await;
        Self {
            pool,
            org,
            provider,
        }
    }

    async fn org(pool: &PgPool, slug: &str) -> Uuid {
        organizations::create(pool, slug, "Org", now())
            .await
            .expect("create organization")
            .id
    }

    async fn provider_in(pool: &PgPool, org: Uuid, key: &str) -> IdentityProvider {
        sqlx::query(
            "INSERT INTO identity_providers (id, org_id, key, kind, config)
             VALUES ($1, $2, $3, 'oidc', '{}'::jsonb)",
        )
        .bind(new_id())
        .bind(org)
        .bind(key)
        .execute(pool)
        .await
        .expect("insert provider");
        identity_providers::get_by_key(pool, scope(org), key)
            .await
            .expect("read provider")
            .expect("the provider exists")
    }

    fn context(&self) -> Context<'static> {
        Context {
            scope: scope(self.org),
            client: ClientAddress(CLIENT),
            now: now(),
            keys: &KEY_RING,
            providers: &PROVIDERS,
        }
    }

    async fn allow_signup(&self, allowed: bool) {
        sqlx::query(
            "UPDATE organizations
             SET settings = settings || jsonb_build_object('admission',
                                jsonb_build_object('signup_allowed', $2::boolean))
             WHERE id = $1",
        )
        .bind(self.org)
        .bind(allowed)
        .execute(&self.pool)
        .await
        .expect("set sign-up");
    }

    /// A user of the organization with `source`, linked to the provider's `subject`.
    async fn member(&self, subject: &str, source: Source, attrs: Value) -> User {
        let new = NewUser {
            display_name: "Pat Example".into(),
            given_name: "Pat".into(),
            family_name: "Example".into(),
            email: Some(format!("{subject}@example.test")),
            locale: None,
            tz: "UTC".into(),
            source,
            external_id: None,
        };
        let user = users::create(&self.pool, scope(self.org), &new, now())
            .await
            .expect("create user");
        let identity = NewIdentity {
            user_id: user.id,
            provider_id: self.provider.id,
            subject: subject.to_owned(),
            attributes: attributes(attrs),
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

    fn sign_in<'a>(&'a self, subject: &'a str, profile: Profile, attrs: Value) -> Verified<'a> {
        Verified {
            provider: &self.provider,
            subject,
            profile,
            email_verified: false,
            attributes: attributes(attrs),
        }
    }

    /// Admit `sign_in` in a transaction that commits whatever admission answered, as a callback
    /// that deletes its flow does.
    async fn admit(&self, sign_in: &Verified<'_>) -> Admission {
        let mut tx = self.pool.begin().await.expect("begin");
        let admission = admission::admit(&mut tx, sign_in, &self.context())
            .await
            .expect("admission answers");
        tx.commit().await.expect("commit");
        admission
    }

    async fn admitted(&self, sign_in: &Verified<'_>) -> User {
        match self.admit(sign_in).await {
            Admission::Admitted(user) => user,
            Admission::Refused(refusal) => panic!("refused: {refusal:?}"),
        }
    }

    async fn refused(&self, sign_in: &Verified<'_>) -> (Step, &'static str) {
        match self.admit(sign_in).await {
            Admission::Refused(refusal) => (refusal.step(), refusal.code()),
            Admission::Admitted(user) => panic!("admitted {}", user.id),
        }
    }

    /// How many rows of `table` (`users`, `identities` or `sessions`) the organization has.
    async fn count(&self, table: &str) -> i64 {
        let sql = match table {
            "users" => "SELECT count(*) FROM users WHERE org_id = $1",
            "identities" => "SELECT count(*) FROM identities WHERE org_id = $1",
            "sessions" => {
                "SELECT count(*) FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.org_id = $1"
            }
            other => panic!("no count for {other}"),
        };
        sqlx::query_scalar(sql)
            .bind(self.org)
            .fetch_one(&self.pool)
            .await
            .expect("count")
    }

    async fn identity_of(&self, user: Uuid) -> identities::Identity {
        let mut found = identities::list_for_user(&self.pool, scope(self.org), user)
            .await
            .expect("list identities");
        assert_eq!(found.len(), 1, "one identity");
        found.remove(0)
    }

    async fn user(&self, id: Uuid) -> User {
        users::get(&self.pool, scope(self.org), id)
            .await
            .expect("read user")
            .expect("the user exists")
    }

    async fn suspend(&self, id: Uuid) {
        users::set_status(
            &self.pool,
            scope(self.org),
            id,
            Status::Suspended,
            now(),
            Actor::Instance,
        )
        .await
        .expect("suspend");
    }
}

/// T1 (B4): a known identity whose user is active is admitted as that user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_known_active_identity_is_admitted_as_its_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let other = f.member("sub-other", Source::Local, json!({})).await;
    let admitted = f
        .admitted(&f.sign_in("sub-pat", Profile::default(), json!({})))
        .await;
    assert_eq!(admitted.id, pat.id);
    assert_ne!(admitted.id, other.id);
    assert_eq!(f.count("users").await, 2, "no user is created");
}

/// T1 (B4): the same identity with a suspended user is refused `auth.suspended` at step 1,
/// even with sign-up allowed, and nothing about the identity is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_suspended_user_is_refused_at_step_1(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let pat = f
        .member("sub-pat", Source::Idp, json!({ "rating": "S1" }))
        .await;
    f.suspend(pat.id).await;
    let sign_in = f.sign_in(
        "sub-pat",
        profile(Some("Robin"), None, None),
        json!({ "rating": "S2" }),
    );
    assert_eq!(
        f.refused(&sign_in).await,
        (Step::KnownIdentity, "auth.suspended")
    );
    let identity = f.identity_of(pat.id).await;
    assert_eq!(identity.attributes, attributes(json!({ "rating": "S1" })));
    assert_eq!(f.user(pat.id).await.given_name, "Pat");
    assert_eq!(f.count("users").await, 1);
}

/// T1, spec 08 B7: `complete` makes a session for an admitted user and none for a suspended
/// one, whose refusal answers 403 `auth.suspended` as problem details.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn complete_makes_a_session_only_for_an_admitted_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let sign_in = f.sign_in("sub-pat", Profile::default(), json!({}));
    let mut tx = f.pool.begin().await.expect("begin");
    let completed = admission::complete(&mut tx, &sign_in, &f.context(), &HeaderMap::new())
        .await
        .expect("admitted");
    tx.commit().await.expect("commit");
    assert_eq!(completed.user.id, pat.id);
    assert_eq!(completed.session.session.user_id, pat.id);
    assert_eq!(f.count("sessions").await, 1);

    f.suspend(pat.id).await;
    sqlx::query("DELETE FROM sessions")
        .execute(&f.pool)
        .await
        .expect("clear sessions");
    let mut tx = f.pool.begin().await.expect("begin");
    let refused = admission::complete(&mut tx, &sign_in, &f.context(), &HeaderMap::new()).await;
    tx.commit().await.expect("commit");
    let Err(Completion::Refused(refusal)) = refused else {
        panic!("not refused: {refused:?}");
    };
    let problem = refusal.into_error().problem();
    assert_eq!(
        (problem.status, problem.code.as_str()),
        (403, "auth.suspended")
    );
    assert_eq!(
        f.count("sessions").await,
        0,
        "a suspended user gets no session"
    );
}

/// T6 (B7): a second sign-in with changed attributes replaces the stored ones.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_sign_in_replaces_the_identitys_attributes(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f
        .member(
            "sub-pat",
            Source::Local,
            json!({ "rating": "S1", "subdivision": "USA" }),
        )
        .await;
    f.admitted(&f.sign_in("sub-pat", Profile::default(), json!({ "rating": "S2" })))
        .await;
    let identity = f.identity_of(pat.id).await;
    assert_eq!(identity.attributes, attributes(json!({ "rating": "S2" })));
    assert_eq!(identity.updated_at, now());
}

/// T15 (B20): with sign-up allowed, a new person is created with the identity-provider source,
/// the mapped names and email, the organization's default time zone, and one identity holding
/// the mapped attributes, linked by the new user from the client address.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_a_new_person_is_created_when_sign_up_is_allowed(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    sqlx::query(
        "UPDATE organizations SET settings = settings || '{\"default_tz\": \"Europe/Dublin\"}'
         WHERE id = $1",
    )
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("set the time zone");
    let sign_in = f.sign_in(
        "sub-new",
        profile(Some("Robin"), Some("Doe"), Some("robin@example.test")),
        json!({ "rating": "S1" }),
    );
    let user = f.admitted(&sign_in).await;
    assert_eq!(
        (user.source, user.status, user.display_name.as_str()),
        (Source::Idp, Status::Active, "Robin Doe")
    );
    assert_eq!(
        (user.given_name.as_str(), user.family_name.as_str()),
        ("Robin", "Doe")
    );
    assert_eq!(user.email.as_deref(), Some("robin@example.test"));
    assert_eq!(user.tz, "Europe/Dublin");
    let identity = f.identity_of(user.id).await;
    assert_eq!(
        (identity.provider_id, identity.subject.as_str()),
        (f.provider.id, "sub-new")
    );
    assert_eq!(identity.attributes, attributes(json!({ "rating": "S1" })));

    let entries = audit::list(&f.pool, scope(f.org), 50)
        .await
        .expect("read audit");
    let link = entries
        .iter()
        .find(|entry| entry.action == "identity.link")
        .expect("an identity.link entry");
    assert_eq!(
        link.actor,
        Actor::User {
            id: user.id,
            ip: CLIENT
        }
    );
    assert_eq!(link.data["after"]["subject"], "sub-new");
    assert!(link.data["after"].get("attributes").is_none());

    let again = f.admitted(&sign_in).await;
    assert_eq!(
        again.id, user.id,
        "the second sign-in is the known identity"
    );
    assert_eq!(f.count("users").await, 1);
}

/// T15 (B20): with both names absent, the display name is the subject.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t15_without_names_the_display_name_is_the_subject(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let user = f
        .admitted(&f.sign_in("1000001", Profile::default(), json!({})))
        .await;
    assert_eq!(user.display_name, "1000001");
    assert_eq!(
        (user.given_name.as_str(), user.family_name.as_str()),
        ("", "")
    );
    assert_eq!(user.email, None);
}

/// T16 (B21): with sign-up off, a new person is refused `auth.signup_closed` and no user or
/// identity row exists afterwards, though the caller's transaction commits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t16_a_new_person_is_refused_when_sign_up_is_off(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let sign_in = f.sign_in("sub-new", profile(Some("Robin"), None, None), json!({}));
    assert_eq!(
        f.refused(&sign_in).await,
        (Step::JustInTime, "auth.signup_closed")
    );
    f.allow_signup(true).await;
    f.allow_signup(false).await;
    assert_eq!(
        f.refused(&sign_in).await,
        (Step::JustInTime, "auth.signup_closed")
    );
    assert_eq!(f.count("users").await, 0);
    assert_eq!(f.count("identities").await, 0);
}

/// B5, B6: with email matching off, a new identity whose email is an existing user's is a new
/// person: refused with sign-up off, and with it on created as a second user without the email
/// (emails are unique), never linked to the existing one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_email_never_links_an_identity(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let email = pat.email.clone().expect("an email");
    let sign_in = f.sign_in("sub-new", profile(None, None, Some(&email)), json!({}));
    assert_eq!(
        f.refused(&sign_in).await,
        (Step::JustInTime, "auth.signup_closed")
    );
    f.allow_signup(true).await;
    let created = f.admitted(&sign_in).await;
    assert_ne!(created.id, pat.id);
    assert_eq!(created.email, None);
    assert_eq!(f.identity_of(pat.id).await.subject, "sub-pat");
}

/// B20, spec 10 B10: a full plan refuses the new person with `entitlement.limit_reached`, and
/// nothing is created.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_full_plan_refuses_just_in_time_creation(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    f.member("sub-pat", Source::Local, json!({})).await;
    let plan = new_id();
    sqlx::query(
        "INSERT INTO plans (id, key, features, limits) VALUES ($1, $2, '{}', '{\"users\": 1}')",
    )
    .bind(plan)
    .bind(format!("test_{}", plan.simple()))
    .execute(&f.pool)
    .await
    .expect("insert plan");
    sqlx::query("UPDATE org_plans SET plan_id = $2, source = 'grant' WHERE org_id = $1")
        .bind(f.org)
        .bind(plan)
        .execute(&f.pool)
        .await
        .expect("move to the plan");
    let sign_in = f.sign_in("sub-new", Profile::default(), json!({}));
    let Admission::Refused(refusal) = f.admit(&sign_in).await else {
        panic!("admitted past the limit");
    };
    assert_eq!(
        (refusal.step(), refusal.code()),
        (Step::JustInTime, "entitlement.limit_reached")
    );
    let problem = refusal.into_error().problem();
    assert_eq!((problem.status, problem.max), (403, Some(1)));
    assert_eq!(f.count("users").await, 1);
    // A known user still signs in on a full plan: the limit counts new users only.
    f.admitted(&f.sign_in("sub-pat", Profile::default(), json!({})))
        .await;
}

/// B3, fail closed: a fault after the user is inserted (attributes PostgreSQL cannot store)
/// is an error, never an admission, and the savepoint leaves no user behind even when the
/// caller commits.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_fault_admits_no_one_and_leaves_nothing(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let sign_in = f.sign_in("sub-new", Profile::default(), json!({ "bad": "a\u{0}b" }));
    let mut tx = f.pool.begin().await.expect("begin");
    let answer = admission::admit(&mut tx, &sign_in, &f.context()).await;
    let error = answer.expect_err("a fault is an error");
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    tx.commit().await.expect("commit");
    assert_eq!(f.count("users").await, 0);
    assert_eq!(f.count("identities").await, 0);
}

/// A local sign-in whose account has no local identity is a fault, even with sign-up allowed:
/// a local sign-in never creates a user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_sign_in_never_creates_a_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let local = identity_providers::get_by_key(&f.pool, scope(f.org), "local")
        .await
        .expect("read")
        .expect("the local row");
    let subject = new_id().to_string();
    let sign_in = Verified {
        provider: &local,
        subject: &subject,
        profile: Profile::default(),
        email_verified: false,
        attributes: Map::new(),
    };
    let mut tx = f.pool.begin().await.expect("begin");
    let answer = admission::admit(&mut tx, &sign_in, &f.context()).await;
    assert_eq!(
        answer.expect_err("a fault").status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    drop(tx);
    assert_eq!(f.count("users").await, 0);
}

/// A provider disabled since the caller read it admits no one.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_disabled_provider_admits_no_one(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.member("sub-pat", Source::Local, json!({})).await;
    sqlx::query("UPDATE identity_providers SET enabled = false WHERE id = $1")
        .bind(f.provider.id)
        .execute(&f.pool)
        .await
        .expect("disable");
    let mut tx = f.pool.begin().await.expect("begin");
    let sign_in = f.sign_in("sub-pat", Profile::default(), json!({}));
    let answer = admission::admit(&mut tx, &sign_in, &f.context()).await;
    assert!(answer.is_err(), "{answer:?}");
}

/// Tenancy: organization A's provider row in organization B's sign-in is a fault, with sign-up
/// off or on: it neither admits A's user nor creates a user in B.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_provider_admits_no_one(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.member("sub-pat", Source::Local, json!({})).await;
    let org_b = Fixture::org(&f.pool, "org-b").await;
    let b = Fixture {
        pool: f.pool.clone(),
        org: org_b,
        provider: Fixture::provider_in(&f.pool, org_b, "sso").await,
    };
    let context = b.context();
    for allowed in [false, true] {
        b.allow_signup(allowed).await;
        for subject in ["sub-pat", "sub-new"] {
            let sign_in = f.sign_in(subject, Profile::default(), json!({}));
            let mut tx = f.pool.begin().await.expect("begin");
            let answer = admission::admit(&mut tx, &sign_in, &context).await;
            let error = answer.expect_err("another organization's provider is a fault");
            assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    assert_eq!(b.count("users").await, 0);
    assert_eq!(b.count("identities").await, 0);
}

/// Wait until a backend of this database waits for a lock, for at most 5 seconds. Returns
/// whether one did; a caller whose code under test takes no lock sees `false`.
async fn wait_for_a_lock_wait(pool: &PgPool) -> bool {
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting > 0 {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// B4, step 1's lock: a suspension holding the user's row makes `admit` wait, and once it
/// commits the sign-in is refused `auth.suspended`. Read without the lock, the sign-in would
/// see the user still active and be admitted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn step_1_waits_for_a_suspension_and_refuses(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let mut holder = f.pool.begin().await.expect("begin");
    users::set_status(
        &mut *holder,
        scope(f.org),
        pat.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend, uncommitted");
    let (pool, provider, context) = (f.pool.clone(), f.provider.clone(), f.context());
    let admitting = tokio::spawn(async move {
        let sign_in = Verified {
            provider: &provider,
            subject: "sub-pat",
            profile: Profile::default(),
            email_verified: false,
            attributes: Map::new(),
        };
        let mut tx = pool.begin().await.expect("begin");
        let answer = admission::admit(&mut tx, &sign_in, &context).await;
        tx.commit().await.expect("commit");
        answer
    });
    let waited = wait_for_a_lock_wait(&f.pool).await;
    holder.commit().await.expect("commit the suspension");
    let answer = admitting
        .await
        .expect("admit runs")
        .expect("admission answers");
    let Admission::Refused(refusal) = answer else {
        panic!("admitted a user suspended before the lock was granted");
    };
    assert_eq!(refusal.code(), "auth.suspended");
    assert!(waited, "admit did not wait for the suspension's lock");
}

/// The same lock for `complete_local`: a passkey sign-in waits for the suspension and makes no
/// session.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_sign_in_waits_for_a_suspension_and_refuses(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let mut holder = f.pool.begin().await.expect("begin");
    users::set_status(
        &mut *holder,
        scope(f.org),
        pat.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend, uncommitted");
    let (pool, org) = (f.pool.clone(), f.org);
    let signing_in = tokio::spawn(async move {
        let mut tx = pool.begin().await.expect("begin");
        let answer =
            admission::complete_local(&mut tx, scope(org), pat.id, now(), &HeaderMap::new()).await;
        tx.commit().await.expect("commit");
        answer.map(|_| ())
    });
    let waited = wait_for_a_lock_wait(&f.pool).await;
    holder.commit().await.expect("commit the suspension");
    let answer = signing_in.await.expect("the sign-in runs");
    assert!(
        matches!(answer, Err(Completion::Refused(ref r)) if r.code() == "auth.suspended"),
        "{answer:?}"
    );
    assert!(waited, "the sign-in did not wait for the suspension's lock");
    assert_eq!(f.count("sessions").await, 0);
}

/// B20: two first sign-ins of one subject at once make one user. The second waits on the
/// identity's unique index, finds the first's identity once it commits, and is admitted as the
/// same user; its own user insert is rolled back.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_first_sign_ins_at_once_make_one_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let mut first = f.pool.begin().await.expect("begin");
    let sign_in = f.sign_in("sub-new", Profile::default(), json!({}));
    let Admission::Admitted(created) = admission::admit(&mut first, &sign_in, &f.context())
        .await
        .expect("admission answers")
    else {
        panic!("the first sign-in is refused");
    };
    let (pool, provider, context) = (f.pool.clone(), f.provider.clone(), f.context());
    let second = tokio::spawn(async move {
        let sign_in = Verified {
            provider: &provider,
            subject: "sub-new",
            profile: Profile::default(),
            email_verified: false,
            attributes: Map::new(),
        };
        let mut tx = pool.begin().await.expect("begin");
        let answer = admission::admit(&mut tx, &sign_in, &context).await;
        tx.commit().await.expect("commit");
        answer
    });
    assert!(
        wait_for_a_lock_wait(&f.pool).await,
        "the second sign-in did not wait"
    );
    first.commit().await.expect("commit the first");
    let answer = second
        .await
        .expect("admit runs")
        .expect("admission answers");
    let Admission::Admitted(user) = answer else {
        panic!("the second sign-in is refused: {answer:?}");
    };
    assert_eq!(user.id, created.id);
    assert_eq!(f.count("users").await, 1);
    assert_eq!(f.count("identities").await, 1);
}

/// Q6 (b): an `idp` user's profile follows the provider of their oldest identity; a field
/// the provider leaves out is kept, and an email another user has is not taken.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q6_an_idp_users_profile_follows_their_provider(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.allow_signup(true).await;
    let first = f.sign_in(
        "sub-new",
        profile(Some("Robin"), Some("Doe"), Some("robin@example.test")),
        json!({}),
    );
    let robin = f.admitted(&first).await;
    let renamed = f.sign_in("sub-new", profile(Some("Rob"), None, None), json!({}));
    let user = f.admitted(&renamed).await;
    assert_eq!(
        (
            user.given_name.as_str(),
            user.family_name.as_str(),
            user.display_name.as_str()
        ),
        ("Rob", "Doe", "Rob Doe")
    );
    assert_eq!(user.email.as_deref(), Some("robin@example.test"));
    assert_eq!(
        f.user(robin.id).await,
        user,
        "the answer is the stored user"
    );

    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let taken = pat.email.clone().expect("an email");
    let moved = f.sign_in("sub-new", profile(None, None, Some(&taken)), json!({}));
    let user = f.admitted(&moved).await;
    assert_eq!(user.email.as_deref(), Some("robin@example.test"));
}

/// Q6 (b): a user an admin made keeps their profile, and so does an `idp` user signing in
/// through an identity that is not their oldest.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn q6_other_users_keep_their_profile(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f.member("sub-pat", Source::Local, json!({})).await;
    let changed = profile(Some("Robin"), Some("Doe"), Some("robin@example.test"));
    let user = f
        .admitted(&f.sign_in("sub-pat", changed.clone(), json!({})))
        .await;
    assert_eq!(user, pat, "a local user is not updated");

    let idp = f.member("sub-idp", Source::Idp, json!({})).await;
    let second = Fixture::provider_in(&f.pool, f.org, "other").await;
    let identity = NewIdentity {
        user_id: idp.id,
        provider_id: second.id,
        subject: "sub-second".into(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link a second identity");
    let sign_in = Verified {
        provider: &second,
        subject: "sub-second",
        profile: changed,
        email_verified: false,
        attributes: Map::new(),
    };
    let user = f.admitted(&sign_in).await;
    assert_eq!(user, idp, "a second provider does not own the profile");
}

/// A log buffer for one test's `tracing` events.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// T17 (B2): the refusal redirect carries only `error=<code>`, and the log line names the
/// provider and the step and holds no subject, profile value or attribute value.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t17_a_refusal_says_only_its_code_and_logs_no_personal_data(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter("stepclimb_server=trace")
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let pat = f
        .member("sub-secret-pat", Source::Idp, json!({ "rating": "S1" }))
        .await;
    f.suspend(pat.id).await;
    let sign_in = f.sign_in(
        "sub-secret-pat",
        profile(Some("Hidden-Given"), None, Some("hidden@example.test")),
        json!({ "rating": "rating-value-S3" }),
    );
    let Admission::Refused(refusal) = f.admit(&sign_in).await else {
        panic!("admitted a suspended user");
    };
    let redirect = refusal.redirect();
    assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        redirect.headers()[header::LOCATION],
        "/sign-in?error=auth.suspended"
    );
    let new = f.sign_in(
        "sub-secret-new",
        profile(Some("Hidden-Given"), None, None),
        json!({ "rating": "rating-value-S3" }),
    );
    f.refused(&new).await;

    let log = String::from_utf8(captured.0.lock().expect("log").clone()).expect("UTF-8");
    let lines: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("admission refused"))
        .collect();
    assert_eq!(lines.len(), 2, "{log}");
    assert!(lines[0].contains("provider=\"sso\""), "{log}");
    assert!(lines[0].contains("step=\"known_identity\""), "{log}");
    assert!(lines[1].contains("step=\"just_in_time\""), "{log}");
    for secret in [
        "sub-secret",
        "Hidden-Given",
        "hidden@example",
        "rating-value",
    ] {
        assert!(!log.contains(secret), "{secret} in {log}");
    }
}
