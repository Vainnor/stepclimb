// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sign-in flows over HTTP headers (docs/specs/m0/08-sessions-and-csrf.md B17 to B21, Q8; T18,
//! T19; spec m1/01 T14). The tests stand in for the sign-in steps of specs 04 to 07 that will call
//! `flow::start` and `flow::finish`. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, header};
use sqlx::PgPool;
use stepclimb_auth::{encryption::KeyRing, token::TokenHash};
use stepclimb_core::{
    OrgScope,
    auth_flows::{Kind, LIFETIME},
    clock::ManualClock,
    id::{Uuid, new_id},
};
use stepclimb_jobs::{
    Worker,
    backoff::Random,
    schedule::{self, Tick},
};
use stepclimb_server::flow::{self, FinishError, Finished, FlowStart, Started};

const PAYLOAD: &[u8] = b"{\"pkce_verifier\":\"zq-verifier-31\",\"nonce\":\"zq-nonce-77\"}";

/// 2025-11-13T02:13:20Z.
fn start_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the request-organization resolver of the sign-in specs"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn keys() -> KeyRing {
    KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").expect("a valid test key")
}

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

struct Fixture {
    pool: PgPool,
    clock: ManualClock,
    keys: KeyRing,
    org: Uuid,
}

impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let org = insert_org(&pool, "org-a").await;
        Self {
            pool,
            clock: ManualClock::new(start_time()),
            keys: keys(),
            org,
        }
    }

    async fn start(&self, kind: Kind, return_to: Option<&str>) -> Started {
        let flow = FlowStart {
            kind,
            user_id: None,
            provider_id: None,
            payload: PAYLOAD,
            return_to,
        };
        flow::start(
            &self.pool,
            &self.keys,
            scope(self.org),
            flow,
            self.clock.clock().now(),
        )
        .await
        .expect("start a flow")
    }

    /// The continuing step, in its own committed transaction, as the browser sends `cookie`.
    async fn finish(&self, cookie: Option<&str>, kind: Kind) -> Result<Finished, FinishError> {
        self.finish_in(self.org, cookie, kind).await
    }

    async fn finish_in(
        &self,
        org: Uuid,
        cookie: Option<&str>,
        kind: Kind,
    ) -> Result<Finished, FinishError> {
        let mut tx = self.pool.begin().await.expect("begin");
        let result = flow::finish(
            &mut *tx,
            &self.keys,
            scope(org),
            &headers(cookie),
            kind,
            self.clock.clock().now(),
        )
        .await;
        tx.commit().await.expect("commit");
        result
    }

    async fn flow_count(&self) -> i64 {
        flow_count(&self.pool).await
    }
}

async fn flow_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM auth_flows")
        .fetch_one(pool)
        .await
        .expect("count flows")
}

fn headers(cookie: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(cookie) = cookie {
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(cookie).expect("a valid Cookie header"),
        );
    }
    headers
}

/// The `name=value` pair a browser sends back for a `Set-Cookie`.
fn cookie_pair(started: &Started) -> String {
    let set_cookie = started.set_cookie.to_str().expect("an ASCII Set-Cookie");
    let (pair, _attributes) = set_cookie
        .split_once(';')
        .expect("attributes follow the pair");
    pair.to_owned()
}

/// The `Max-Age` the `Set-Cookie` gives the browser.
fn max_age(started: &Started) -> Duration {
    let set_cookie = started.set_cookie.to_str().expect("an ASCII Set-Cookie");
    let seconds = set_cookie
        .split("; ")
        .find_map(|attribute| attribute.strip_prefix("Max-Age="))
        .expect("a Max-Age attribute")
        .parse()
        .expect("whole seconds");
    Duration::from_secs(seconds)
}

fn token(started: &Started) -> String {
    cookie_pair(started)
        .strip_prefix("__Host-sc_flow=")
        .expect("the flow cookie's name")
        .to_owned()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_started_in_a_browser_finishes_there_once(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::Oidc, Some("/c/s1/m/2")).await;
    assert_eq!(started.expires_at, start_time() + LIFETIME);
    let set_cookie = started.set_cookie.to_str().unwrap();
    assert!(
        set_cookie.ends_with("; Max-Age=1200; Path=/; Secure; HttpOnly; SameSite=Lax"),
        "{set_cookie}"
    );
    let cookie = format!("theme=dark; {}", cookie_pair(&started));

    let finished = fixture
        .finish(Some(&cookie), Kind::Oidc)
        .await
        .expect("the first use completes the flow");
    assert_eq!(finished.id, started.id);
    assert_eq!(finished.payload, PAYLOAD);
    assert_eq!(finished.return_to.as_str(), "/c/s1/m/2");
    assert_eq!(fixture.flow_count().await, 0);

    let again = fixture.finish(Some(&cookie), Kind::Oidc).await;
    assert!(matches!(again, Err(FinishError::Invalid)), "T18: {again:?}");
    assert_eq!(again.unwrap_err().code(), Some("auth.flow_invalid"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_used_after_its_lifetime_has_expired(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::PasskeySignIn, None).await;
    let cookie = cookie_pair(&started);
    // The browser still holds the cookie one second before its Max-Age runs out, which is past
    // the flow's lifetime, so the late user reaches the server's expiry check.
    let late_but_held = max_age(&started) - Duration::from_secs(1);
    assert!(late_but_held >= LIFETIME, "the cookie outlives the flow");
    fixture.clock.advance(late_but_held);

    let late = fixture.finish(Some(&cookie), Kind::PasskeySignIn).await;

    assert!(matches!(late, Err(FinishError::Expired)), "T18: {late:?}");
    assert_eq!(late.unwrap_err().code(), Some("auth.flow_expired"));
    assert_eq!(
        fixture.flow_count().await,
        0,
        "B21: the finding request deletes it"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_just_inside_its_lifetime_finishes(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::PasskeySignIn, None).await;
    fixture.clock.advance(LIFETIME - Duration::from_secs(1));

    let finished = fixture
        .finish(Some(&cookie_pair(&started)), Kind::PasskeySignIn)
        .await;

    assert!(finished.is_ok(), "{finished:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn return_to_keeps_only_a_path_on_this_host(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let cases = [
        (Some("https://evil.example"), "/"),
        (Some("//evil.example"), "/"),
        (Some("/\\evil.example"), "/"),
        (Some("javascript:x"), "/"),
        (Some("/\t/evil.example"), "/"),
        (None, "/"),
        (Some("/c/s1/m/2"), "/c/s1/m/2"),
    ];
    for (requested, kept) in cases {
        let started = fixture.start(Kind::Oidc, requested).await;
        assert_eq!(started.return_to.as_str(), kept, "T19: {requested:?}");
        let finished = fixture
            .finish(Some(&cookie_pair(&started)), Kind::Oidc)
            .await
            .unwrap();
        assert_eq!(finished.return_to.as_str(), kept, "T19: {requested:?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_request_without_exactly_one_matching_flow_cookie_is_invalid(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::Oidc, None).await;
    let pair = cookie_pair(&started);
    let other_browser = fixture.start(Kind::Oidc, None).await;

    for cookie in [
        None,
        Some("theme=dark".to_owned()),
        Some("__Host-sc_flow=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned()),
        Some(format!("{pair}; {pair}")),
        Some(format!("x{pair}")),
        Some(format!("__Host-sc_session={}", token(&started))),
    ] {
        let result = fixture.finish(cookie.as_deref(), Kind::Oidc).await;
        assert!(
            matches!(result, Err(FinishError::Invalid)),
            "{cookie:?}: {result:?}"
        );
    }
    assert_eq!(
        fixture.flow_count().await,
        2,
        "no refused request deletes a flow"
    );

    // The other browser's cookie completes the other browser's flow, not this one.
    let theirs = fixture
        .finish(Some(&cookie_pair(&other_browser)), Kind::Oidc)
        .await
        .unwrap();
    assert_eq!(theirs.id, other_browser.id);
    assert!(fixture.finish(Some(&pair), Kind::Oidc).await.is_ok());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_finishes_only_for_its_own_kind_and_organization(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let other_org = insert_org(&fixture.pool, "org-b").await;
    let started = fixture.start(Kind::Oauth2, None).await;
    let cookie = cookie_pair(&started);

    let wrong_kind = fixture.finish(Some(&cookie), Kind::Oidc).await;
    assert!(
        matches!(wrong_kind, Err(FinishError::Invalid)),
        "{wrong_kind:?}"
    );
    let wrong_org = fixture
        .finish_in(other_org, Some(&cookie), Kind::Oauth2)
        .await;
    assert!(
        matches!(wrong_org, Err(FinishError::Invalid)),
        "{wrong_org:?}"
    );

    assert!(fixture.finish(Some(&cookie), Kind::Oauth2).await.is_ok());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_database_holds_neither_the_token_nor_the_payload_in_clear(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::Oidc, None).await;

    let (token_hash, payload): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT token_hash, payload FROM auth_flows WHERE id = $1")
            .bind(started.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();

    assert_eq!(token_hash, TokenHash::of(&token(&started)).as_bytes());
    let contains = |haystack: &[u8], needle: &[u8]| {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    };
    assert!(
        !contains(&payload, b"zq-verifier-31"),
        "B20: the payload is sealed"
    );
    assert!(!contains(&payload, b"zq-nonce-77"));
    assert!(!contains(&payload, token(&started).as_bytes()));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_payload_copied_to_another_flow_does_not_open(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let source = fixture.start(Kind::Oidc, None).await;
    let target = fixture.start(Kind::Oidc, None).await;
    sqlx::query(
        "UPDATE auth_flows SET payload = (SELECT payload FROM auth_flows WHERE id = $1)
         WHERE id = $2",
    )
    .bind(source.id)
    .bind(target.id)
    .execute(&fixture.pool)
    .await
    .unwrap();

    let result = fixture
        .finish(Some(&cookie_pair(&target)), Kind::Oidc)
        .await;

    assert!(matches!(result, Err(FinishError::Decrypt(_))), "{result:?}");
    assert_eq!(
        result.unwrap_err().code(),
        None,
        "a server fault, not a user error"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn cleanup_deletes_expired_flows_in_every_organization(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let other_org = insert_org(&fixture.pool, "org-b").await;
    fixture.start(Kind::Oidc, None).await;
    flow::start(
        &fixture.pool,
        &fixture.keys,
        scope(other_org),
        FlowStart {
            kind: Kind::Oidc,
            user_id: None,
            provider_id: None,
            payload: PAYLOAD,
            return_to: None,
        },
        start_time(),
    )
    .await
    .unwrap();
    fixture.clock.advance(Duration::from_secs(60));
    let live = fixture.start(Kind::Oidc, None).await;

    let at_first_expiry = start_time() + LIFETIME;
    let deleted = flow::delete_expired(&fixture.pool, at_first_expiry)
        .await
        .unwrap();

    assert_eq!(deleted, 2);
    let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM auth_flows")
        .fetch_all(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(left, [live.id]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_flow_moved_to_another_organization_does_not_open_there(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let other_org = insert_org(&fixture.pool, "org-b").await;
    let started = fixture.start(Kind::Oidc, None).await;
    sqlx::query("UPDATE auth_flows SET org_id = $1 WHERE id = $2")
        .bind(other_org)
        .bind(started.id)
        .execute(&fixture.pool)
        .await
        .unwrap();

    let result = fixture
        .finish_in(other_org, Some(&cookie_pair(&started)), Kind::Oidc)
        .await;

    assert!(matches!(result, Err(FinishError::Decrypt(_))), "{result:?}");
}

// Spec m1/01 T14, B18; spec 08 B21: the scheduler enqueues `auth.flows_cleanup`, and the job
// deletes the flow that has expired.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_cleanup_job_deletes_an_expired_flow(pool: PgPool) {
    let fixture = Fixture::new(pool).await;
    let started = fixture.start(Kind::Oidc, None).await;
    fixture.clock.advance(LIFETIME);
    let now = fixture.clock.clock().now();
    let registry = stepclimb_server::jobs::registry().expect("the registry builds");
    schedule::sync(&fixture.pool, &registry, now).await.unwrap();
    let tick = schedule::tick(&fixture.pool, &registry, now).await.unwrap();
    assert!(
        matches!(&tick, Tick::Ran(names) if names.iter().any(|n| n == "auth.flows_cleanup")),
        "{tick:?}"
    );
    let queues = registry.queues();
    let worker = Worker::new(
        fixture.pool.clone(),
        registry,
        fixture.clock.clock(),
        Random::from_fn(|| 0),
        queues,
    );
    worker.run_once(10).await.unwrap();
    assert_eq!(
        flow_count(&fixture.pool).await,
        0,
        "flow {} is still stored",
        started.id
    );
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "both periodic jobs ran and are done");
}
