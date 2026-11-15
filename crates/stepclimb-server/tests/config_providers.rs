// SPDX-License-Identifier: AGPL-3.0-or-later
//! The providers of `stepclimb.toml` written at startup (`stepclimb_server::config_providers`;
//! docs/specs/m0/06-oauth2-and-vatsim-preset.md B11, B12, Q2, T9, T10; `05-oidc.md` Q2 (c)),
//! and a sign-in through one, against the mock IdP's `/oauth2` provider on 127.0.0.1. Needs
//! `DATABASE_URL` (`just up`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension,
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use stepclimb_auth::{
    encryption::KeyRing,
    outbound::{OutboundClient, UrlPolicy},
    provider_config::{ConfigSecrets, ProviderRow},
};
use stepclimb_core::{
    OrgScope, Source,
    clock::ManualClock,
    id::Uuid,
    identities, organizations,
    users::{self, NewUser},
};
use stepclimb_mock_idp::{Endpoint, MockIdp};
use stepclimb_server::{
    AppState, Config,
    client_ip::TrustedProxies,
    config::{ConfigFile, Env},
    config_providers::{self, ApplyError},
    providers::Providers,
    rate_limit::{Limits, RateLimiter},
    request_org::TEST_ORG_HEADER,
    router,
    startup::StartupError,
    web_origin::WebOrigin,
};
use tower::ServiceExt;
use url::Url;

const CLIENT: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)), 50_000);
const KEY_RING: &str = "test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const PUBLIC_URL: &str = "https://lms.example.org";

fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn restart() -> SystemTime {
    start() + Duration::from_secs(3_600)
}

/// The configuration `toml` gives, with the insecure switch on so the mock's `http` URLs pass.
fn config(toml: &str) -> Config {
    let env: Env = [
        ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
        ("STEPCLIMB_ENCRYPTION_KEYS", KEY_RING),
        ("STEPCLIMB_PUBLIC_URL", PUBLIC_URL),
        ("STEPCLIMB_ALLOW_INSECURE_PROVIDERS", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let file = ConfigFile::parse(
        PathBuf::from("/etc/stepclimb/stepclimb.toml"),
        toml.to_owned(),
    )
    .expect("parses");
    Config::load(&env, Some(&file)).expect("valid")
}

/// An `oidc` entry with `key`, `extra` lines appended.
fn oidc(key: &str, extra: &str) -> String {
    format!(
        "[[providers]]\nkind = \"oidc\"\nkey = \"{key}\"\nlabel = \"Sign in with {key}\"\n\
         issuer = \"https://idp.example.test/{key}\"\nclient_id = \"stepclimb\"\n\
         client_secret = \"secret-of-{key}\"\n{extra}\n"
    )
}

/// An `oauth2` entry pointing at `mock` with its client, as the key `toml`.
fn mock_entry(mock: &MockIdp, secret: &str) -> String {
    format!(
        "[[providers]]\nkind = \"oauth2\"\nkey = \"toml\"\nlabel = \"Sign in from the file\"\n\
         authorize = \"{}\"\ntoken = \"{}\"\nuserinfo = \"{}\"\nclient_id = \"{}\"\n\
         client_secret = \"{secret}\"\n\n[providers.claims]\nsubject = \"/data/cid\"\n",
        mock.url(Endpoint::OAuth2Authorize),
        mock.url(Endpoint::OAuth2Token),
        mock.url(Endpoint::OAuth2Userinfo),
        mock.client_id(),
    )
}

async fn org(pool: &PgPool, slug: &str) -> Uuid {
    organizations::create(pool, slug, slug, start())
        .await
        .expect("create organization")
        .id
}

async fn apply(pool: &PgPool, toml: &str, now: SystemTime) -> Result<ConfigSecrets, ApplyError> {
    config_providers::apply(pool, &config(toml).providers, true, now).await
}

/// `(org_id, key, kind, enabled, config, updated_at in seconds)` of every outside provider.
async fn rows(pool: &PgPool) -> Vec<(Uuid, String, String, bool, Value, i64)> {
    sqlx::query_as(
        "SELECT org_id, key, kind, enabled, config, extract(epoch FROM updated_at)::bigint
         FROM identity_providers WHERE kind <> 'local' ORDER BY org_id, key",
    )
    .fetch_all(pool)
    .await
    .expect("read providers")
}

fn row_of(secrets: &ConfigSecrets, org_id: Uuid, provider_id: Uuid) -> Option<String> {
    secrets
        .get(ProviderRow {
            org_id,
            provider_id,
        })
        .map(|s| s.expose_secret().to_owned())
}

async fn id_of(pool: &PgPool, org_id: Uuid, key: &str) -> Uuid {
    sqlx::query_scalar("SELECT id FROM identity_providers WHERE org_id = $1 AND key = $2")
        .bind(org_id)
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the row")
}

/// Link a new synthetic user of `org` to the provider `provider_id` by `subject`.
async fn link(pool: &PgPool, org: Uuid, provider_id: Uuid, subject: &str) {
    let user = stepclimb_core::id::new_id();
    sqlx::query(
        "INSERT INTO users (id, org_id, display_name, source, status, tz)
         VALUES ($1, $2, 'Test Student', 'idp', 'active', 'UTC')",
    )
    .bind(user)
    .bind(org)
    .execute(pool)
    .await
    .expect("a user");
    sqlx::query(
        "INSERT INTO identities (id, org_id, user_id, provider_id, subject)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(stepclimb_core::id::new_id())
    .bind(org)
    .bind(user)
    .bind(provider_id)
    .bind(subject)
    .execute(pool)
    .await
    .expect("an identity");
}

/// An API row with an identity, as the admin API would leave it.
async fn api_row_with_identity(pool: &PgPool, org: Uuid) -> Uuid {
    let api = stepclimb_core::id::new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'corp', 'oidc',
                 '{\"issuer\": \"https://idp.example.test/corp\", \"label\": \"API\", \"client_secret\": {\"sealed\": \"AAAA\"}}',
                 true)",
    )
    .bind(api)
    .bind(org)
    .execute(pool)
    .await
    .expect("an API row");
    link(pool, org, api, "1000001").await;
    api
}

/// The refusal `apply` gives, which must stop startup with exit 2.
fn refusal(result: Result<ConfigSecrets, ApplyError>) -> String {
    let error = result.expect_err("refused");
    let message = error.to_string();
    assert_eq!(StartupError::from(error).exit_code(), 2, "{message}");
    message
}

/// Spec 06 B13 decision 5 (revised): the file does not take over an API row its users sign in
/// with; the row is left as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_file_does_not_take_over_an_api_row_with_identities(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    api_row_with_identity(&pool, a).await;
    let before = rows(&pool).await;

    let message = refusal(apply(&pool, &oidc("corp", ""), start()).await);

    assert_eq!(
        message,
        "stepclimb.toml [[providers]] entry 1 (key \"corp\"): would take over provider \"corp\" \
         of organization \"org-a\", created through the admin API, which 1 identities sign in with"
    );
    assert_eq!(rows(&pool).await, before, "nothing is written");
}

/// Decision 5 (revised), between restarts: a file row with identities keeps its issuer and
/// kind; a label change still goes through.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_row_with_identities_keeps_its_issuer_and_kind(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    apply(&pool, &oidc("corp", ""), start())
        .await
        .expect("applies");
    link(&pool, a, id_of(&pool, a, "corp").await, "1000001").await;
    let before = rows(&pool).await;

    let moved = oidc("corp", "").replace(
        "https://idp.example.test/corp",
        "https://evil.example.test/corp",
    );
    let message = refusal(apply(&pool, &moved, restart()).await);
    assert!(
        message.ends_with(
            "`issuer` differs from provider \"corp\" of organization \"org-a\", which 1 \
             identities sign in with; keep the stored value or remove those identities first"
        ),
        "{message}"
    );
    assert_eq!(rows(&pool).await, before, "nothing is written");

    let oauth2 = "[[providers]]\nkind = \"oauth2\"\nkey = \"corp\"\nlabel = \"Corp\"\n\
        authorize = \"https://idp.example.test/a\"\ntoken = \"https://idp.example.test/t\"\n\
        userinfo = \"https://idp.example.test/u\"\nclient_id = \"c\"\nclient_secret = \"s\"\n\n\
        [providers.claims]\nsubject = \"/sub\"\n";
    let message = refusal(apply(&pool, oauth2, restart()).await);
    assert!(
        message.contains("`kind` differs from provider \"corp\""),
        "{message}"
    );

    let relabeled = oidc("corp", "").replace("Sign in with corp", "Sign in with Corp SSO");
    apply(&pool, &relabeled, restart())
        .await
        .expect("a label is not an anchor");
}

/// Decision 5 (revised) for `oauth2`: the endpoints' origins and the subject pointer.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_oauth2_file_row_with_identities_keeps_its_origins_and_subject(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let mock = mock().await;
    let file = mock_entry(&mock, "s");
    apply(&pool, &file, start()).await.expect("applies");
    link(&pool, a, id_of(&pool, a, "toml").await, "1000001").await;

    let token = mock.url(Endpoint::OAuth2Token);
    let elsewhere = file.replace(&token, "http://127.0.0.2:9/oauth2/token");
    let message = refusal(apply(&pool, &elsewhere, restart()).await);
    assert!(
        message.contains("`token` differs from provider \"toml\""),
        "{message}"
    );

    let subject = file.replace("subject = \"/data/cid\"", "subject = \"/data/email\"");
    let message = refusal(apply(&pool, &subject, restart()).await);
    assert!(
        message.contains("`claims.subject` differs from provider \"toml\""),
        "{message}"
    );
}

/// T10: on a single-organization instance a provider without `org` is written to the one
/// organization, marked `source: "config"`, with no secret in the row; the secret is held in
/// memory for that row.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_provider_is_written_to_the_only_organization_without_its_secret(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let secrets = apply(&pool, &oidc("corp", ""), start())
        .await
        .expect("applies");

    let rows = rows(&pool).await;
    let [(org_id, key, kind, enabled, config, _)] = rows.as_slice() else {
        panic!("one row: {rows:?}");
    };
    assert_eq!(
        (*org_id, key.as_str(), kind.as_str(), *enabled),
        (a, "corp", "oidc", true)
    );
    assert_eq!(config["source"], json!("config"));
    assert_eq!(config["label"], json!("Sign in with corp"));
    assert_eq!(config.get("client_secret"), None);
    assert!(!config.to_string().contains("secret-of-corp"), "{config}");
    let id = id_of(&pool, a, "corp").await;
    assert_eq!(secrets.len(), 1);
    assert_eq!(row_of(&secrets, a, id).as_deref(), Some("secret-of-corp"));
}

/// T10: a restart with the same file changes nothing; one without the entry disables the row
/// and keeps it and its identities; putting it back enables it again.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn removing_a_provider_from_the_file_disables_it_and_keeps_its_identities(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let file = format!("{}{}", oidc("corp", ""), oidc("kept", ""));
    apply(&pool, &file, start()).await.expect("applies");
    let corp = id_of(&pool, a, "corp").await;
    let user = stepclimb_core::id::new_id();
    sqlx::query(
        "INSERT INTO users (id, org_id, display_name, source, status, tz)
         VALUES ($1, $2, 'Test Student', 'idp', 'active', 'UTC')",
    )
    .bind(user)
    .bind(a)
    .execute(&pool)
    .await
    .expect("a user");
    sqlx::query(
        "INSERT INTO identities (id, org_id, user_id, provider_id, subject)
         VALUES ($1, $2, $3, $4, '1000001')",
    )
    .bind(stepclimb_core::id::new_id())
    .bind(a)
    .bind(user)
    .bind(corp)
    .execute(&pool)
    .await
    .expect("an identity");

    let before = rows(&pool).await;
    apply(&pool, &file, restart()).await.expect("again");
    assert_eq!(rows(&pool).await, before, "the same file changes nothing");

    let secrets = apply(&pool, &oidc("kept", ""), restart())
        .await
        .expect("without corp");
    let after = rows(&pool).await;
    assert_eq!(after.len(), 2, "nothing is deleted: {after:?}");
    let corp_row = after.iter().find(|r| r.1 == "corp").expect("corp stays");
    assert!(!corp_row.3, "corp is disabled");
    assert_eq!(
        corp_row.5,
        i64::try_from(
            restart()
                .duration_since(UNIX_EPOCH)
                .expect("after the epoch")
                .as_secs()
        )
        .expect("fits")
    );
    assert!(after.iter().find(|r| r.1 == "kept").expect("kept").3);
    assert_eq!(
        row_of(&secrets, a, corp),
        None,
        "no secret for a provider the file dropped"
    );
    let identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE provider_id = $1")
            .bind(corp)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(identities, 1);

    apply(&pool, &file, restart()).await.expect("corp is back");
    let back = rows(&pool).await;
    assert!(
        back.iter().find(|r| r.1 == "corp").expect("corp").3,
        "enabled again"
    );
    assert_eq!(id_of(&pool, a, "corp").await, corp, "the same row");
}

/// Spec 07 I3: a row's admission rules are not the file's to set, so a restart keeps the rules
/// an admin gave the provider, whatever the file says.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_restart_keeps_the_rows_admission_rules(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    apply(&pool, &oidc("corp", ""), start())
        .await
        .expect("applies");
    let rules = json!([{ "attribute": "subdivision", "op": "equals", "value": "USA" }]);
    sqlx::query(
        "UPDATE identity_providers SET config = config || jsonb_build_object('admission_rules', $3::jsonb)
         WHERE org_id = $1 AND key = $2",
    )
    .bind(a)
    .bind("corp")
    .bind(&rules)
    .execute(&pool)
    .await
    .expect("rules set");

    let changed = oidc("corp", "").replace("Sign in with corp", "Sign in with Corp SSO");
    apply(&pool, &changed, restart())
        .await
        .expect("applies again");

    let rows = rows(&pool).await;
    let [(_, _, _, _, config, _)] = rows.as_slice() else {
        panic!("one row: {rows:?}");
    };
    assert_eq!(
        config["label"],
        json!("Sign in with Corp SSO"),
        "the file's change is written"
    );
    assert_eq!(config["admission_rules"], rules, "the rules are kept");
    assert_eq!(config["source"], json!("config"));
}

/// Q2: the file owns the providers it declares, so a row the admin API wrote with the same key
/// is taken over; its sealed secret is dropped and `enabled = false` in the file wins.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_file_takes_over_a_row_with_its_key(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let api = stepclimb_core::id::new_id();
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'corp', 'oauth2', '{\"label\": \"API\", \"client_secret\": {\"sealed\": \"AAAA\"}}', true)",
    )
    .bind(api)
    .bind(a)
    .execute(&pool)
    .await
    .expect("an API row");

    let file = oidc("corp", "enabled = false");
    let (secrets, log) = logged(apply(&pool, &file, start())).await;
    let secrets = secrets.expect("applies");
    assert!(
        log.contains(
            "provider \"corp\" was created through the admin API; stepclimb.toml now owns it"
        ),
        "{log}"
    );
    assert!(
        log.contains("provider \"corp\" changes kind")
            && log.contains("from=\"oauth2\"")
            && log.contains("to=\"oidc\""),
        "{log}"
    );
    assert!(!log.contains("secret-of-corp"), "{log}");

    // The row is the file's now, and its kind the file's: a restart warns about neither.
    let (again, log) = logged(apply(&pool, &file, restart())).await;
    again.expect("applies again");
    assert!(!log.contains("admin API"), "{log}");
    assert!(!log.contains("changes kind"), "{log}");
    let rows = rows(&pool).await;
    let [(_, _, kind, enabled, config, _)] = rows.as_slice() else {
        panic!("one row: {rows:?}");
    };
    assert_eq!((kind.as_str(), *enabled), ("oidc", false));
    assert_eq!(config["source"], json!("config"));
    assert_eq!(config.get("client_secret"), None);
    assert_eq!(row_of(&secrets, a, api).as_deref(), Some("secret-of-corp"));
}

/// A writer that keeps every log line, for the startup warnings.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("not poisoned")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `work`'s result and everything it logged, as text. The subscriber is this thread's while
/// `work` runs; `#[sqlx::test]` runs a current-thread runtime.
async fn logged<T>(work: impl std::future::Future<Output = T>) -> (T, String) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let result = work.await;
    drop(guard);
    let text = String::from_utf8(captured.0.lock().expect("not poisoned").clone()).expect("UTF-8");
    (result, text)
}

/// Q2 (a), second half: with two organizations an entry must name one by its slug.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn several_organizations_need_org_and_an_unknown_slug_is_refused(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    let b = org(&pool, "org-b").await;

    let error = apply(&pool, &oidc("corp", ""), start()).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "stepclimb.toml [[providers]] entry 1 (key \"corp\"): `org` is required on an instance \
         with more than one organization"
    );
    assert_eq!(StartupError::from(error).exit_code(), 2);

    let error = apply(&pool, &oidc("corp", "org = \"org-c\""), start())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .ends_with("`org` names no organization of this instance (\"org-c\")"),
        "{error}"
    );
    assert!(
        rows(&pool).await.is_empty(),
        "a refused file writes nothing"
    );

    let file = format!(
        "{}{}",
        oidc("corp", "org = \"org-b\"").replace("secret-of-corp", "secret-of-b"),
        oidc("corp", "org = \"org-a\"").replace("secret-of-corp", "secret-of-a")
    );
    let secrets = apply(&pool, &file, start()).await.expect("applies");
    let rows = rows(&pool).await;
    let mut orgs: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    orgs.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(orgs, expected);
    assert_eq!(secrets.len(), 2);
    let (in_a, in_b) = (id_of(&pool, a, "corp").await, id_of(&pool, b, "corp").await);
    assert_eq!(row_of(&secrets, a, in_a).as_deref(), Some("secret-of-a"));
    assert_eq!(row_of(&secrets, b, in_b).as_deref(), Some("secret-of-b"));
    assert_eq!(
        row_of(&secrets, a, in_b),
        None,
        "a row is found only in its own organization"
    );
    assert_eq!(row_of(&secrets, b, in_a), None);
}

/// An entry naming the only organization and one naming none both land there: one key, twice.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_same_key_twice_in_one_organization_is_refused(pool: PgPool) {
    org(&pool, "org-a").await;
    let file = format!("{}{}", oidc("corp", ""), oidc("corp", "org = \"org-a\""));
    let error = apply(&pool, &file, start()).await.unwrap_err();
    assert!(
        error.to_string().ends_with(
            "entry 2 (key \"corp\"): `key` is also the key of entry 1 for the same organization"
        ),
        "{error}"
    );
    assert!(rows(&pool).await.is_empty());
}

/// `05-oidc.md` Q4: private addresses only on a single-organization instance; the file is
/// checked again under the instance's policy.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn private_addresses_are_refused_once_there_are_two_organizations(pool: PgPool) {
    org(&pool, "org-a").await;
    let private = oidc("corp", "org = \"org-a\"").replace(
        "https://idp.example.test/corp",
        "https://10.1.2.3/realms/corp",
    );
    apply(&pool, &private, start())
        .await
        .expect("one organization");
    org(&pool, "org-b").await;
    let error = apply(&pool, &private, restart()).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("`issuer` is refused by the outbound URL policy"),
        "{error}"
    );
    assert_eq!(StartupError::from(error).exit_code(), 2);
}

/// A fresh instance has no organization to give the providers to: nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_instance_without_an_organization_writes_nothing(pool: PgPool) {
    let secrets = apply(&pool, &oidc("corp", "org = \"org-a\""), start())
        .await
        .expect("goes on");
    assert!(secrets.is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM identity_providers")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(count, 0);
}

#[derive(Debug)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
}

impl Answer {
    fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }

    fn flow_cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("__Host-sc_flow=") && !value.contains("Max-Age=0"))
            .map(|value| value.split(';').next().unwrap_or("").to_owned())
    }

    fn signed_in(&self) -> bool {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.starts_with("__Host-sc_session="))
    }
}

async fn get(app: &axum::Router, org: Uuid, path: &str, cookie: Option<&str>) -> Answer {
    let mut request = Request::get(path).header(TEST_ORG_HEADER, org.to_string());
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("a request"))
        .await
        .expect("an answer");
    Answer {
        status: response.status(),
        headers: response.headers().clone(),
    }
}

/// Sign in through the file's `toml` provider, with the app holding `secrets`: the callback's
/// answer.
async fn sign_in(pool: &PgPool, org: Uuid, secrets: ConfigSecrets) -> Answer {
    let state = AppState {
        db: pool.clone(),
        clock: ManualClock::new(start()).clock(),
        keys: KeyRing::parse(KEY_RING).expect("a key"),
        public_url: WebOrigin::parse(PUBLIC_URL).expect("an origin"),
        allowed_origins: Vec::new(),
        rate_limits: RateLimiter::new(Limits::DEFAULT, TrustedProxies::none()),
        providers: Providers::new(true)
            .expect("clients")
            .with_config_secrets(secrets),
        passwords: Default::default(),
    };
    let app = router(state).layer(Extension(ConnectInfo(CLIENT)));
    let started = get(&app, org, "/auth/toml/start?return_to=/c/s1", None).await;
    assert_eq!(started.status, StatusCode::SEE_OTHER, "{started:?}");
    let cookie = started.flow_cookie().expect("the flow cookie");
    let client = OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client");
    let request = axum::http::Request::get(started.location())
        .body(Vec::new())
        .expect("a request");
    let response = client.send(request).await.expect("the mock answers");
    let back = Url::parse(
        response.headers()[header::LOCATION]
            .to_str()
            .expect("an ASCII Location"),
    )
    .expect("an absolute redirect");
    let query = back.query().expect("code and state").to_owned();
    get(
        &app,
        org,
        &format!("/auth/toml/callback?{query}"),
        Some(&cookie),
    )
    .await
}

async fn mock() -> MockIdp {
    let mock = MockIdp::start().await.expect("the mock starts");
    mock.set_oauth2_userinfo(Some(json!({ "data": { "cid": 1000001 } })));
    mock
}

async fn allow_signup(pool: &PgPool, org: Uuid) {
    sqlx::query(
        "UPDATE organizations SET settings = settings || '{\"admission\": {\"signup_allowed\": true}}'
         WHERE id = $1",
    )
    .bind(org)
    .execute(pool)
    .await
    .expect("sign-up on");
}

/// The secret read from the file reaches the token request: the mock authenticates the client
/// with it, and the sign-in completes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_sign_in_through_a_file_provider_uses_the_secret_from_the_file(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    allow_signup(&pool, a).await;
    let mock = mock().await;
    let secrets = apply(&pool, &mock_entry(&mock, &mock.client_secret()), start())
        .await
        .expect("applies");

    let answer = sign_in(&pool, a, secrets).await;

    assert_eq!(answer.status, StatusCode::SEE_OTHER, "{answer:?}");
    assert_eq!(answer.location(), "/c/s1", "{answer:?}");
    assert!(answer.signed_in(), "{answer:?}");
    assert_eq!(mock.requests_to(Endpoint::OAuth2Token).len(), 1);
}

/// A wrong secret in the file is the one sent: the mock refuses the client.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_wrong_secret_in_the_file_fails_the_token_request(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    allow_signup(&pool, a).await;
    let mock = mock().await;
    let secrets = apply(&pool, &mock_entry(&mock, "not-the-mock-secret"), start())
        .await
        .expect("applies");

    let answer = sign_in(&pool, a, secrets).await;

    assert_eq!(
        answer.location(),
        "/sign-in?error=auth.provider_error",
        "{answer:?}"
    );
    assert!(!answer.signed_in());
}

/// Finding 3: another server, reading another file, rewrote the row after this one started.
/// This server's secret was read for the old config, so the callback answers 500 and sends no
/// token request.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_row_rewritten_from_another_file_gets_no_secret_here(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    allow_signup(&pool, a).await;
    let mock = mock().await;
    let file = mock_entry(&mock, &mock.client_secret());
    let ours = apply(&pool, &file, start()).await.expect("this server");
    let theirs = file.replace("Sign in from the file", "Sign in from another file");
    apply(&pool, &theirs, restart())
        .await
        .expect("another server");

    let answer = sign_in(&pool, a, ours).await;

    assert_eq!(
        answer.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{answer:?}"
    );
    assert!(!answer.signed_in());
    assert!(
        mock.requests_to(Endpoint::OAuth2Token).is_empty(),
        "no secret was sent"
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test stands in for the admin route that creates the user"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Spec 07 I4: `trusted_for_email_matching = true` in the file reaches the stored row, and a
/// sign-in through that provider whose verified email is an existing user's is linked to that
/// user (email matching on, sign-up off). The mapping names `email` and `email_verified`.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_provider_trusted_for_matching_links_by_email(pool: PgPool) {
    let a = org(&pool, "org-a").await;
    sqlx::query("UPDATE organizations SET settings = settings || '{\"email_matching\": true}' WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .expect("matching on");
    let scope = scope(a);
    let new = NewUser {
        display_name: "Robin Example".into(),
        given_name: String::new(),
        family_name: String::new(),
        email: Some("robin@example.test".into()),
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    };
    let robin = users::create(&pool, scope, &new, start())
        .await
        .expect("create user");
    let mock = mock().await;
    mock.set_oauth2_userinfo(Some(json!({
        "data": { "cid": 1000001, "email": "Robin@example.test", "verified": true }
    })));
    let file = mock_entry(&mock, &mock.client_secret()).replace(
        "\n[providers.claims]\n",
        "trusted_for_email_matching = true\n\n[providers.claims]\n",
    ) + "email = \"/data/email\"\nemail_verified = \"/data/verified\"\n";
    let secrets = apply(&pool, &file, start()).await.expect("applies");
    let rows = rows(&pool).await;
    assert_eq!(rows[0].4["trusted_for_email_matching"], json!(true));

    let answer = sign_in(&pool, a, secrets).await;

    assert!(answer.signed_in(), "{answer:?}");
    let linked = identities::list_for_user(&pool, scope, robin.id)
        .await
        .expect("list identities");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].subject, "1000001");
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE org_id = $1")
        .bind(a)
        .fetch_one(&pool)
        .await
        .expect("count users");
    assert_eq!(users, 1);
}
