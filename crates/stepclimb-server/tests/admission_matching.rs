// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unknown-identity resolution and sync-only mode against the database
//! (docs/specs/m0/07-admission.md B5, B6, B11, I4; T2 to T5, T9; spec 03 Q2 (a)). The tests call
//! `admission::admit` as a provider's callback does, on a transaction of their own, with
//! provider rows inserted by SQL: `trusted`, whose config sets `trusted_for_email_matching`,
//! and `plain`, whose config does not. Every condition of an email match is tested on its own,
//! with the others met, so that each one is seen to refuse. All data is synthetic.
//!
//! Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::{self, Actor},
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider},
    organizations,
    users::{self, NewUser, Status, User},
};
use stepclimb_server::{
    admission::{self, Admission, Context, Profile, Step, Verified},
    audit::ClientAddress,
};

/// The client address of every sign-in (TEST-NET-1).
const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));

/// The address the existing user and the signing-in person share.
const EMAIL: &str = "robin@example.test";

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the routes that resolve the request's organization"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn with_email(email: &str) -> Profile {
    Profile {
        email: Some(email.to_owned()),
        ..Profile::default()
    }
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    /// Trusted for matching.
    trusted: IdentityProvider,
    /// Not trusted for matching.
    plain: IdentityProvider,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = Self::org(&pool, "org-a").await;
        let trusted = provider_in(
            &pool,
            org,
            "trusted",
            json!({ "trusted_for_email_matching": true }),
        )
        .await;
        let plain = provider_in(&pool, org, "plain", json!({})).await;
        let f = Self {
            pool,
            org,
            trusted,
            plain,
        };
        f.set(json!({ "email_matching": true })).await;
        f
    }

    async fn org(pool: &PgPool, slug: &str) -> Uuid {
        organizations::create(pool, slug, "Org", now())
            .await
            .expect("create organization")
            .id
    }

    /// Merge `settings` into the organization's stored settings, by SQL.
    async fn set(&self, settings: Value) {
        sqlx::query("UPDATE organizations SET settings = settings || $2 WHERE id = $1")
            .bind(self.org)
            .bind(settings)
            .execute(&self.pool)
            .await
            .expect("store settings");
    }

    async fn admission(&self, signup_allowed: bool, sync_only: bool) {
        let admission = json!({ "signup_allowed": signup_allowed, "sync_only": sync_only });
        self.set(json!({ "admission": admission })).await;
    }

    fn context(&self) -> Context {
        Context {
            scope: scope(self.org),
            client: ClientAddress(CLIENT),
            now: now(),
        }
    }

    /// A user of the organization with `source` and `email`, without identities. A local user's
    /// email is recorded as verified by `users::create`; any other user's is not.
    async fn user(&self, source: Source, email: &str) -> User {
        let new = NewUser {
            display_name: "Robin Example".into(),
            given_name: "Robin".into(),
            family_name: "Example".into(),
            email: Some(email.to_owned()),
            locale: None,
            tz: "UTC".into(),
            source,
            external_id: None,
        };
        users::create(&self.pool, scope(self.org), &new, now())
            .await
            .expect("create user")
    }

    /// The stored record of `user`'s verified email.
    async fn verified_email(&self, user: Uuid) -> Option<String> {
        sqlx::query_scalar("SELECT verified_email FROM users WHERE org_id = $1 AND id = $2")
            .bind(self.org)
            .bind(user)
            .fetch_one(&self.pool)
            .await
            .expect("read the record")
    }

    fn sign_in<'a>(
        &self,
        provider: &'a IdentityProvider,
        subject: &'a str,
        email: &str,
        email_verified: bool,
    ) -> Verified<'a> {
        Verified {
            provider,
            subject,
            profile: with_email(email),
            email_verified,
            attributes: Map::new(),
        }
    }

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

    /// The subjects of the identities `user` has, by provider key.
    async fn identities_of(&self, user: Uuid) -> Vec<(String, String)> {
        identities::list_for_user(&self.pool, scope(self.org), user)
            .await
            .expect("list identities")
            .into_iter()
            .map(|identity| (identity.provider_key, identity.subject))
            .collect()
    }

    async fn users(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM users WHERE org_id = $1")
            .bind(self.org)
            .fetch_one(&self.pool)
            .await
            .expect("count users")
    }
}

/// A provider of `org` with `config`, inserted by SQL: the provider routes are spec 06's.
async fn provider_in(pool: &PgPool, org: Uuid, key: &str, config: Value) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config)
         VALUES ($1, $2, $3, 'oidc', $4)",
    )
    .bind(new_id())
    .bind(org)
    .bind(key)
    .bind(config)
    .execute(pool)
    .await
    .expect("insert provider");
    identity_providers::get_by_key(pool, scope(org), key)
        .await
        .expect("read provider")
        .expect("the provider exists")
}

const SIGNUP_CLOSED: (Step, &str) = (Step::JustInTime, "auth.signup_closed");

/// T4 (B5, spec 03 Q2 (a)): with matching on, a verified email through a trusted provider links
/// the new identity to the local user with that address, in any letter case. The link's audit
/// entry names the user as the actor, from the request's address, and no user is created.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_a_verified_email_through_a_trusted_provider_links_the_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let sign_in = f.sign_in(&f.trusted, "sub-robin", "Robin@Example.TEST", true);

    let admitted = f.admitted(&sign_in).await;

    assert_eq!(admitted.id, robin.id);
    assert_eq!(
        admitted.email.as_deref(),
        Some(EMAIL),
        "a local user keeps their profile"
    );
    assert_eq!(
        f.identities_of(robin.id).await,
        [("trusted".to_owned(), "sub-robin".to_owned())]
    );
    assert_eq!(f.users().await, 1);
    let entries = audit::list(&f.pool, scope(f.org), 10).await.expect("list");
    let link = entries
        .iter()
        .find(|entry| entry.action == "identity.link")
        .expect("a link entry");
    assert_eq!(
        link.actor,
        Actor::User {
            id: robin.id,
            ip: CLIENT
        }
    );

    let again = f.admitted(&sign_in).await;
    assert_eq!(again.id, robin.id, "the second sign-in is a known identity");
    assert_eq!(f.identities_of(robin.id).await.len(), 1);
}

/// T4, unmet: an email the provider did not mark verified is never matched; the person is new
/// (sign-up off: refused) and nothing is linked.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_an_unverified_email_is_not_matched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, false);
    assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED);
    assert!(f.identities_of(robin.id).await.is_empty());
}

/// T4, unmet: a provider whose config does not set `trusted_for_email_matching` to exactly
/// `true` is never trusted, whatever it says about the address.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_a_provider_not_trusted_for_matching_never_matches(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let sign_in = f.sign_in(&f.plain, "sub-robin", EMAIL, true);
    assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED);
    for (key, trust) in [
        ("off", json!(false)),
        ("text", json!("true")),
        ("one", json!(1)),
    ] {
        let config = json!({ "trusted_for_email_matching": trust });
        let provider = provider_in(&f.pool, f.org, key, config).await;
        let sign_in = f.sign_in(&provider, "sub-robin", EMAIL, true);
        assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED, "{key}");
    }
    assert!(f.identities_of(robin.id).await.is_empty());
}

/// T4, unmet: trust is read from the provider row as stored at the sign-in, not from the copy
/// the callback read earlier.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_trust_is_read_from_the_stored_provider(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    identity_providers::replace_config(&f.pool, scope(f.org), f.trusted.id, &Map::new(), now())
        .await
        .expect("withdraw the trust");
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED);
    assert!(f.identities_of(robin.id).await.is_empty());
}

/// T3 (B5, B6): with matching off, every other condition met, the identity is not linked.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_with_matching_off_nothing_is_linked_by_email(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.set(json!({ "email_matching": false })).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED);
    f.admission(true, false).await;
    let created = f.admitted(&sign_in).await;
    assert_ne!(created.id, robin.id);
    assert_eq!(created.email, None, "the address is the existing user's");
    assert!(f.identities_of(robin.id).await.is_empty());
}

/// The stored side (coordinator decision): a user whose email is not recorded as verified is
/// never a candidate. Here an `idp` user created just-in-time through an untrusted provider,
/// which stores the address unverified; once a trusted, verified sign-in recorded it, the same
/// user is matched.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_email_not_recorded_as_verified_is_never_matched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.admission(true, false).await;
    let first = f.sign_in(&f.plain, "sub-plain", EMAIL, true);
    let squatter = f.admitted(&first).await;
    assert_eq!(squatter.email.as_deref(), Some(EMAIL));
    assert_eq!(f.verified_email(squatter.id).await, None);
    f.admission(false, false).await;

    let victim = f.sign_in(&f.trusted, "sub-trusted", EMAIL, true);
    assert_eq!(f.refused(&victim).await, SIGNUP_CLOSED);
    assert_eq!(f.identities_of(squatter.id).await.len(), 1);

    users::record_verified_email(&f.pool, scope(f.org), squatter.id, EMAIL)
        .await
        .expect("record");
    assert_eq!(f.admitted(&victim).await.id, squatter.id);
}

/// The outside-source exclusion (coordinator decision): a user a sync, SCIM or a connector owns
/// is never matched, even with a verified email recorded; a local or `idp` one is.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_an_outside_source_owns_is_never_matched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    for (n, source) in [Source::Sync, Source::Scim, Source::Connector]
        .into_iter()
        .enumerate()
    {
        let email = format!("outside{n}@example.test");
        let user = f.user(source, &email).await;
        users::record_verified_email(&f.pool, scope(f.org), user.id, &email)
            .await
            .expect("record");
        assert_eq!(f.verified_email(user.id).await, Some(email.clone()));
        let subject = format!("sub-{n}");
        let sign_in = f.sign_in(&f.trusted, &subject, &email, true);
        assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED, "{source:?}");
        assert!(f.identities_of(user.id).await.is_empty(), "{source:?}");
    }
    let idp = f.user(Source::Idp, EMAIL).await;
    users::record_verified_email(&f.pool, scope(f.org), idp.id, EMAIL)
        .await
        .expect("record");
    let sign_in = f.sign_in(&f.trusted, "sub-idp", EMAIL, true);
    assert_eq!(f.admitted(&sign_in).await.id, idp.id);
}

/// B4 for a match: a suspended user matched by email is refused `auth.suspended`, even with
/// sign-up allowed, so a suspended person does not get a second account; nothing is linked or
/// created.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_suspended_match_is_refused(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.admission(true, false).await;
    let robin = f.user(Source::Local, EMAIL).await;
    users::set_status(
        &f.pool,
        scope(f.org),
        robin.id,
        Status::Suspended,
        now(),
        Actor::Instance,
    )
    .await
    .expect("suspend");
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    assert_eq!(
        f.refused(&sign_in).await,
        (Step::KnownIdentity, "auth.suspended")
    );
    assert!(f.identities_of(robin.id).await.is_empty());
    assert_eq!(f.users().await, 1);
}

/// T2 (B3) and the refusal marker: a refusal by the rules after an email match leaves no
/// identity behind, and marks the matched user as a known user's refusal does.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_later_refusal_leaves_no_link(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let rule = json!({ "attribute": "subdivision", "op": "equals", "value": "USA" });
    f.set(json!({ "admission": { "rules": [rule] } })).await;
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);

    assert_eq!(
        f.refused(&sign_in).await,
        (Step::Rules, "auth.admission_refused")
    );

    assert!(f.identities_of(robin.id).await.is_empty());
    let marker: Option<String> = sqlx::query_scalar(
        "SELECT admission_refused_provider FROM users WHERE org_id = $1 AND id = $2",
    )
    .bind(f.org)
    .bind(robin.id)
    .fetch_one(&f.pool)
    .await
    .expect("read the marker");
    assert_eq!(marker.as_deref(), Some("trusted"));
}

/// T9 (B11): in sync-only mode a new person is refused `auth.not_provisioned` even with sign-up
/// allowed, and no user is created; a user matched by email is still admitted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_sync_only_refuses_new_people_and_admits_users(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.admission(true, true).await;
    let stranger = f.sign_in(&f.trusted, "sub-new", "new@example.test", true);
    assert_eq!(
        f.refused(&stranger).await,
        (Step::SyncOnly, "auth.not_provisioned")
    );
    assert_eq!(f.users().await, 0);

    let robin = f.user(Source::Local, EMAIL).await;
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    assert_eq!(f.admitted(&sign_in).await.id, robin.id);

    f.admission(true, false).await;
    assert_ne!(f.admitted(&stranger).await.id, robin.id);
}

/// T5 (Q4 (b)): a synced user an admin pre-provisioned an identity for signs in as that user
/// in sync-only mode, the first time, with matching off.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_a_pre_provisioned_identity_signs_in_a_synced_user(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.set(json!({ "email_matching": false })).await;
    f.admission(false, true).await;
    let synced = f.user(Source::Sync, "synced@example.test").await;
    let identity = NewIdentity {
        user_id: synced.id,
        provider_id: f.plain.id,
        subject: "1000001".into(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("pre-provision");
    let sign_in = f.sign_in(&f.plain, "1000001", "other@example.test", false);
    assert_eq!(f.admitted(&sign_in).await.id, synced.id);
}

/// The record of a verified email at just-in-time creation: kept only for a trusted provider
/// that marked the address verified.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn just_in_time_records_only_a_vouched_email(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.admission(true, false).await;
    let cases = [
        (&f.trusted, true, true),
        (&f.trusted, false, false),
        (&f.plain, true, false),
    ];
    for (n, (provider, verified, recorded)) in cases.into_iter().enumerate() {
        let (subject, email) = (format!("sub-{n}"), format!("Person{n}@Example.test"));
        let created = f
            .admitted(&f.sign_in(provider, &subject, &email, verified))
            .await;
        let expected = recorded.then(|| email.to_lowercase());
        assert_eq!(f.verified_email(created.id).await, expected, "case {n}");
    }
}

/// Q6 and the record: a new address from the provider that owns an `idp` user's profile drops
/// the record unless that provider is trusted and marked it verified, and a verified sign-in
/// through it records an address that was unverified.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_profile_update_keeps_the_record_true_to_its_source(pool: PgPool) {
    let f = Fixture::new(pool).await;
    f.admission(true, false).await;
    let pat = f
        .admitted(&f.sign_in(&f.trusted, "sub-pat", EMAIL, false))
        .await;
    assert_eq!(f.verified_email(pat.id).await, None);

    f.admitted(&f.sign_in(&f.trusted, "sub-pat", EMAIL, true))
        .await;
    assert_eq!(f.verified_email(pat.id).await, Some(EMAIL.to_owned()));

    let moved = "moved@example.test";
    let user = f
        .admitted(&f.sign_in(&f.trusted, "sub-pat", moved, false))
        .await;
    assert_eq!(user.email.as_deref(), Some(moved));
    assert_eq!(f.verified_email(pat.id).await, None);

    let sam = f
        .admitted(&f.sign_in(&f.plain, "sub-sam", "sam@example.test", true))
        .await;
    assert_eq!(
        f.verified_email(sam.id).await,
        None,
        "an untrusted provider records nothing"
    );
}

/// Two first sign-ins of one subject matched to the same user at once link one identity: the
/// second waits on the user's row lock, then finds the identity the first linked and is
/// admitted as the same user.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_matching_first_sign_ins_at_once_link_once(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let mut first = f.pool.begin().await.expect("begin");
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    let Admission::Admitted(linked) = admission::admit(&mut first, &sign_in, &f.context())
        .await
        .expect("admission answers")
    else {
        panic!("the first sign-in is refused");
    };
    assert_eq!(linked.id, robin.id);
    let (pool, provider, context) = (f.pool.clone(), f.trusted.clone(), f.context());
    let second = tokio::spawn(async move {
        let sign_in = Verified {
            provider: &provider,
            subject: "sub-robin",
            profile: with_email(EMAIL),
            email_verified: true,
            attributes: Map::new(),
        };
        let mut tx = pool.begin().await.expect("begin");
        let answer = admission::admit(&mut tx, &sign_in, &context).await;
        tx.commit().await.expect("commit");
        answer
    });
    assert!(
        wait_for_a_lock_wait(&f.pool).await,
        "the second did not wait"
    );
    first.commit().await.expect("commit the first");
    let answer = second
        .await
        .expect("admit runs")
        .expect("admission answers");
    let Admission::Admitted(user) = answer else {
        panic!("the second sign-in is refused: {answer:?}");
    };
    assert_eq!(user.id, robin.id);
    assert_eq!(f.identities_of(robin.id).await.len(), 1);
}

/// Wait until a backend of this database waits for a lock, for at most 5 seconds.
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

/// Cross-organization (T20 for matching): organization B's user with the same verified email
/// is never matched by organization A's sign-in, and A's matching setting never applies to B.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_user_is_never_matched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let b = Fixture::org(&f.pool, "org-b").await;
    let new = NewUser {
        display_name: "Robin Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: Some(EMAIL.into()),
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let in_b = users::create(&f.pool, scope(b), &new, now())
        .await
        .expect("create B's user");
    let sign_in = f.sign_in(&f.trusted, "sub-robin", EMAIL, true);
    assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED);
    let in_b_identities = identities::list_for_user(&f.pool, scope(b), in_b.id)
        .await
        .expect("list");
    assert!(in_b_identities.is_empty());

    let b_trusted = provider_in(
        &f.pool,
        b,
        "trusted",
        json!({ "trusted_for_email_matching": true }),
    )
    .await;
    let context = Context {
        scope: scope(b),
        client: ClientAddress(CLIENT),
        now: now(),
    };
    let sign_in = f.sign_in(&b_trusted, "sub-robin", EMAIL, true);
    let mut tx = f.pool.begin().await.expect("begin");
    let answer = admission::admit(&mut tx, &sign_in, &context)
        .await
        .expect("admission answers");
    tx.rollback().await.expect("roll back");
    let Admission::Refused(refusal) = answer else {
        panic!("B matched by email with matching off in B");
    };
    assert_eq!(refusal.code(), "auth.signup_closed");
}

/// Coordinator decision (security review of #199): a user who already has an identity at the
/// signing-in provider is not a match candidate for another subject there, so a second account
/// at the same provider claiming the address is a new person; at another trusted provider the
/// same user still matches.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_user_with_an_identity_at_the_provider_is_not_matched_again_there(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let robin = f.user(Source::Local, EMAIL).await;
    let first = f.sign_in(&f.trusted, "sub-first", EMAIL, true);
    assert_eq!(f.admitted(&first).await.id, robin.id);

    let second = f.sign_in(&f.trusted, "sub-second", EMAIL, true);
    assert_eq!(f.refused(&second).await, SIGNUP_CLOSED);
    assert_eq!(f.identities_of(robin.id).await.len(), 1);

    let config = json!({ "trusted_for_email_matching": true });
    let other = provider_in(&f.pool, f.org, "other", config).await;
    let elsewhere = f.sign_in(&other, "sub-elsewhere", EMAIL, true);
    assert_eq!(f.admitted(&elsewhere).await.id, robin.id);
}

/// Coordinator decision (security review of #199): a verified address whose Unicode lowercase
/// equals a user's ASCII address (a Kelvin sign, a dotted capital I) is not matched to that
/// user; ASCII letter case still is.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_unicode_look_alike_address_is_not_matched(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let kim = f.user(Source::Local, "kim@example.test").await;
    let ivy = f.user(Source::Local, "ivy@example.test").await;
    for (n, email) in ["\u{212A}im@example.test", "\u{130}vy@example.test"]
        .into_iter()
        .enumerate()
    {
        let subject = format!("sub-{n}");
        let sign_in = f.sign_in(&f.trusted, &subject, email, true);
        assert_eq!(f.refused(&sign_in).await, SIGNUP_CLOSED, "{email}");
    }
    assert!(f.identities_of(kim.id).await.is_empty());
    assert!(f.identities_of(ivy.id).await.is_empty());
    let ascii = f.sign_in(&f.trusted, "sub-kim", "KIM@example.test", true);
    assert_eq!(f.admitted(&ascii).await.id, kim.id);
}
