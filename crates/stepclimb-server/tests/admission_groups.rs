// SPDX-License-Identifier: AGPL-3.0-or-later
//! Group memberships at sign-in, from the claims and from the admission webhook
//! (docs/specs/m0/07-admission.md B3, B18, B22, Q8 (a); T14, I7). Sign-ins call
//! `admission::admit` as a provider's callback does; the webhook is a receiver on 127.0.0.1
//! whose row is stored directly under the test's key ring. All data is synthetic; nothing leaves
//! the machine. Needs `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{Json, Router, extract::State};
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, webhook::Secret};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    id::{Uuid, new_id},
    identities::{self, NewIdentity},
    identity_providers::{self, IdentityProvider},
    organizations,
    users::{self, NewUser},
};
use stepclimb_server::{
    admission::{self, Admission, Context, Profile, Step, Verified},
    admission_webhook::seal_secret,
    audit::ClientAddress,
    providers::Providers,
};
use tokio::net::TcpListener;

const CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
const KEYS: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

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

fn strings(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|&key| key.to_owned()).collect()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

/// A webhook receiver that answers every call with the reply it is set to.
#[derive(Clone)]
struct Receiver {
    port: u16,
    reply: Arc<Mutex<Value>>,
    calls: Arc<Mutex<usize>>,
}

impl Receiver {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let receiver = Self {
            port: listener.local_addr().expect("an address").port(),
            reply: Arc::new(Mutex::new(json!({ "allow": true }))),
            calls: Arc::default(),
        };
        let app = Router::new()
            .fallback(|State(receiver): State<Receiver>| async move {
                *receiver.calls.lock().expect("calls") += 1;
                Json(receiver.reply.lock().expect("reply").clone())
            })
            .with_state(receiver.clone());
        tokio::spawn(async move { axum::serve(listener, app).await });
        receiver
    }

    fn reply(&self, reply: Value) {
        *self.reply.lock().expect("reply") = reply;
    }

    fn calls(&self) -> usize {
        *self.calls.lock().expect("calls")
    }
}

struct Fixture {
    pool: PgPool,
    org: Uuid,
    keys: KeyRing,
    providers: Providers,
    sso: IdentityProvider,
}

impl Fixture {
    /// Org A with an `oidc` provider `sso` and sign-up allowed.
    async fn new(pool: PgPool) -> Self {
        let org = org(&pool, "org-a").await;
        let sso = provider_in(&pool, org, "sso").await;
        Self {
            pool,
            org,
            keys: KeyRing::parse(KEYS).expect("a valid test key"),
            providers: Providers::new(true).expect("the outbound clients"),
            sso,
        }
    }

    fn context(&self, org: Uuid) -> Context<'_> {
        Context {
            scope: scope(org),
            client: ClientAddress(CLIENT),
            now: now(),
            keys: &self.keys,
            providers: &self.providers,
        }
    }

    /// A sign-in through `provider` as `subject`, with `groups` mapped (`None`: the provider
    /// maps no groups claim).
    fn sign_in<'a>(
        &self,
        provider: &'a IdentityProvider,
        subject: &'a str,
        groups: Option<&[&str]>,
    ) -> Verified<'a> {
        Verified {
            provider,
            subject,
            profile: Profile::default(),
            email_verified: false,
            attributes: Map::new(),
            groups: groups.map(strings),
        }
    }

    /// Admit `sign_in` in `org` in a transaction that commits whatever admission answered.
    async fn admit(&self, org: Uuid, sign_in: &Verified<'_>) -> Admission {
        let mut tx = self.pool.begin().await.expect("begin");
        let admission = admission::admit(&mut tx, sign_in, &self.context(org))
            .await
            .expect("admission answers");
        tx.commit().await.expect("commit");
        admission
    }

    async fn admitted(&self, sign_in: &Verified<'_>) -> Uuid {
        match self.admit(self.org, sign_in).await {
            Admission::Admitted(user) => user.id,
            Admission::Refused(refusal) => panic!("refused: {refusal:?}"),
        }
    }

    /// An enabled webhook of `org` at `receiver`.
    async fn webhook(&self, org: Uuid, receiver: &Receiver) {
        let id = new_id();
        let secret = Secret::generate().expect("a secret");
        let sealed = seal_secret(&self.keys, scope(org), id, &secret).expect("seal");
        sqlx::query(
            "INSERT INTO admission_webhooks (id, org_id, url, secret, enabled)
             VALUES ($1, $2, $3, $4, true)",
        )
        .bind(id)
        .bind(org)
        .bind(format!("http://127.0.0.1:{}/hook", receiver.port))
        .bind(sealed)
        .execute(&self.pool)
        .await
        .expect("insert the webhook");
    }

    /// `user`'s memberships: `(group key, membership source)`, sorted.
    async fn memberships(&self, user: Uuid) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT g.key, m.source FROM group_members m
             JOIN groups g ON g.org_id = m.org_id AND g.id = m.group_id
             WHERE m.user_id = $1 ORDER BY g.key, m.source",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .expect("read memberships")
    }

    async fn group_count(&self, org: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM groups WHERE org_id = $1")
            .bind(org)
            .fetch_one(&self.pool)
            .await
            .expect("count groups")
    }
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    let org = organizations::create(pool, slug, "Org", now())
        .await
        .expect("create organization")
        .id;
    sqlx::query(
        "UPDATE organizations
         SET settings = settings || '{\"admission\": {\"signup_allowed\": true}}'::jsonb
         WHERE id = $1",
    )
    .bind(org)
    .execute(pool)
    .await
    .expect("allow sign-up");
    org
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

/// T14 (B22, Q8 (a)): a new person's claimed groups become `idp` memberships, groups the
/// organization lacks are created, and the next sign-in's claim replaces them: a group left
/// out is removed, a missing claim (an empty list) removes them all.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_claimed_groups_become_memberships_and_follow_the_claim(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home", "staff"])))
        .await;
    assert_eq!(
        f.memberships(pat).await,
        pairs(&[("home", "idp"), ("staff", "idp")])
    );

    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home", "mentor"])))
        .await;
    assert_eq!(
        f.memberships(pat).await,
        pairs(&[("home", "idp"), ("mentor", "idp")])
    );
    assert_eq!(f.group_count(f.org).await, 3);

    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&[]))).await;
    assert!(f.memberships(pat).await.is_empty());
}

/// B22: a provider that maps no groups claim says nothing about groups, so a sign-in through it
/// keeps the claim memberships another sign-in gave.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_provider_that_maps_no_groups_leaves_them_as_they_are(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["staff"])))
        .await;
    f.admitted(&f.sign_in(&f.sso, "sub-pat", None)).await;
    assert_eq!(f.memberships(pat).await, pairs(&[("staff", "idp")]));
}

/// T14 (B18, Q8 (a)): the webhook's groups become `admission_webhook` memberships beside the
/// claim ones; each source replaces only its own, and a reply without `groups` removes the
/// webhook's. A sign-in that does not call the webhook leaves them.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t14_the_webhooks_groups_sit_beside_the_claims(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let receiver = Receiver::start().await;
    f.webhook(f.org, &receiver).await;
    receiver.reply(json!({ "allow": true, "groups": ["staff", "mentor"] }));

    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home", "staff"])))
        .await;
    assert_eq!(
        f.memberships(pat).await,
        pairs(&[
            ("home", "idp"),
            ("mentor", "admission_webhook"),
            ("staff", "admission_webhook"),
            ("staff", "idp"),
        ])
    );

    receiver.reply(json!({ "allow": true, "groups": ["mentor"] }));
    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home"])))
        .await;
    assert_eq!(
        f.memberships(pat).await,
        pairs(&[("home", "idp"), ("mentor", "admission_webhook")])
    );

    sqlx::query("UPDATE admission_webhooks SET enabled = false WHERE org_id = $1")
        .bind(f.org)
        .execute(&f.pool)
        .await
        .expect("disable the webhook");
    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home"])))
        .await;
    assert_eq!(
        f.memberships(pat).await,
        pairs(&[("home", "idp"), ("mentor", "admission_webhook")]),
        "a webhook not called keeps its memberships"
    );

    sqlx::query("UPDATE admission_webhooks SET enabled = true WHERE org_id = $1")
        .bind(f.org)
        .execute(&f.pool)
        .await
        .expect("enable the webhook");
    receiver.reply(json!({ "allow": true }));
    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home"])))
        .await;
    assert_eq!(f.memberships(pat).await, pairs(&[("home", "idp")]));
    assert_eq!(receiver.calls(), 3);
}

/// B3: a refused sign-in changes no membership and creates no group, whether the webhook
/// refuses or cannot decide; a bad key in the reply is a reply it cannot decide with.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_sign_in_changes_no_group(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let receiver = Receiver::start().await;
    f.webhook(f.org, &receiver).await;
    receiver.reply(json!({ "allow": true, "groups": ["mentor"] }));
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home"])))
        .await;
    let before = f.memberships(pat).await;

    for (reply, code) in [
        (
            json!({ "allow": false, "groups": ["staff"] }),
            "auth.admission_refused",
        ),
        (
            json!({ "allow": true, "groups": ["staff", ""] }),
            "auth.admission_unavailable",
        ),
        (
            json!({ "allow": true, "groups": ["a\u{7}b"] }),
            "auth.admission_unavailable",
        ),
    ] {
        receiver.reply(reply);
        let sign_in = f.sign_in(&f.sso, "sub-pat", Some(&["staff"]));
        let Admission::Refused(refusal) = f.admit(f.org, &sign_in).await else {
            panic!("admitted");
        };
        assert_eq!((refusal.step(), refusal.code()), (Step::Webhook, code));
        assert_eq!(f.memberships(pat).await, before);
        assert_eq!(f.group_count(f.org).await, 2, "no group was created");
    }
}

/// Unusable claimed keys are dropped and the sign-in goes on with the rest.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn unusable_claimed_keys_are_dropped(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let long = "k".repeat(256);
    let pat = f
        .admitted(&f.sign_in(
            &f.sso,
            "sub-pat",
            Some(&["staff", "", "a\u{0}b", "x\ny", &long, "staff"]),
        ))
        .await;
    assert_eq!(f.memberships(pat).await, pairs(&[("staff", "idp")]));
}

/// A local sign-in neither calls the webhook nor changes any membership.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_local_sign_in_changes_no_group(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let receiver = Receiver::start().await;
    f.webhook(f.org, &receiver).await;
    receiver.reply(json!({ "allow": true, "groups": ["staff"] }));
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home"])))
        .await;
    let before = f.memberships(pat).await;
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'local', 'local', '{}'::jsonb, true)
         ON CONFLICT (org_id, key) DO NOTHING",
    )
    .bind(new_id())
    .bind(f.org)
    .execute(&f.pool)
    .await
    .expect("a local row");
    let local = identity_providers::get_by_key(&f.pool, scope(f.org), "local")
        .await
        .expect("read provider")
        .expect("the local row");
    let subject = pat.to_string();
    let identity = NewIdentity {
        user_id: pat,
        provider_id: local.id,
        subject: subject.clone(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link the local identity");
    let calls = receiver.calls();

    assert_eq!(f.admitted(&f.sign_in(&local, &subject, None)).await, pat);
    assert_eq!(f.memberships(pat).await, before);
    assert_eq!(receiver.calls(), calls);
}

/// Tenancy: the same key in another organization is that organization's own group, and a
/// sign-in there never changes organization A's groups or memberships.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_sign_in_never_touches_a_groups(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["staff"])))
        .await;
    let b = org(&f.pool, "org-b").await;
    let sso_b = provider_in(&f.pool, b, "sso").await;

    let robin = match f
        .admit(b, &f.sign_in(&sso_b, "sub-pat", Some(&["staff"])))
        .await
    {
        Admission::Admitted(user) => user.id,
        Admission::Refused(refusal) => panic!("refused: {refusal:?}"),
    };
    assert_ne!(robin, pat);
    f.admit(b, &f.sign_in(&sso_b, "sub-pat", Some(&[]))).await;

    assert_eq!(f.memberships(pat).await, pairs(&[("staff", "idp")]));
    assert!(f.memberships(robin).await.is_empty());
    assert_eq!(f.group_count(f.org).await, 1);
    assert_eq!(f.group_count(b).await, 1);
}

/// A known user an admin created (not `idp`) still gets their groups from the claims: groups
/// follow the sign-in, not the user's source.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_pre_provisioned_users_sign_in_brings_its_groups(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let new = NewUser {
        display_name: "Sam Example".into(),
        given_name: "Sam".into(),
        family_name: "Example".into(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let sam = users::create(&f.pool, scope(f.org), &new, now())
        .await
        .expect("create user")
        .id;
    let identity = NewIdentity {
        user_id: sam,
        provider_id: f.sso.id,
        subject: "sub-sam".into(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link identity");

    assert_eq!(
        f.admitted(&f.sign_in(&f.sso, "sub-sam", Some(&["staff"])))
            .await,
        sam
    );
    assert_eq!(f.memberships(sam).await, pairs(&[("staff", "idp")]));
}

/// B22 (spec 07 I7): a sign-in through one provider replaces only that provider's claim
/// memberships; another provider's stay, even for the same group.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_providers_sign_in_replaces_only_its_own_claim_memberships(pool: PgPool) {
    let f = Fixture::new(pool).await;
    let partner = provider_in(&f.pool, f.org, "partner").await;
    let pat = f
        .admitted(&f.sign_in(&f.sso, "sub-pat", Some(&["home", "staff"])))
        .await;
    let identity = NewIdentity {
        user_id: pat,
        provider_id: partner.id,
        subject: "partner-pat".into(),
        attributes: Map::new(),
    };
    identities::link(&f.pool, scope(f.org), &identity, now(), Actor::Instance)
        .await
        .expect("link the second identity");
    f.admitted(&f.sign_in(&partner, "partner-pat", Some(&["staff"])))
        .await;

    f.admitted(&f.sign_in(&f.sso, "sub-pat", Some(&[]))).await;
    assert_eq!(f.memberships(pat).await, pairs(&[("staff", "idp")]));
    let from: String = sqlx::query_scalar(
        "SELECT p.key FROM group_members m JOIN identity_providers p ON p.id = m.provider_id
         WHERE m.user_id = $1",
    )
    .bind(pat)
    .fetch_one(&f.pool)
    .await
    .expect("one membership");
    assert_eq!(from, "partner");
}
