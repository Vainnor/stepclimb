// SPDX-License-Identifier: AGPL-3.0-or-later
//! `[[providers]]` (spec 06 B11, B13; T9).

use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::config::{Config, ConfigError, Env};

const SECRET: &str = "zq-provider-client-secret";
const KEYS: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// A directory holding `vatsim_client_secret` (the secret with blanks around it), removed on
/// drop.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("stepclimb-providers-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vatsim_client_secret"), format!("  {SECRET}\n")).unwrap();
        std::fs::write(dir.join("empty_secret"), "\n").unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// B11's canonical example, with the secret file named relative to the configuration file.
const CANONICAL: &str = r#"
[[providers]]
kind = "oauth2"
key = "vatsim"
label = "Sign in with VATSIM"
authorize = "https://auth.vatsim.net/oauth/authorize"
token = "https://auth.vatsim.net/oauth/token"
userinfo = "https://auth.vatsim.net/api/user"
scopes = ["full_name", "vatsim_details", "email"]
client_id = "stepclimb-test-client"
client_secret_file = "vatsim_client_secret"

[providers.claims]
subject = "/data/cid"
given_name = "/data/personal/name_first"
family_name = "/data/personal/name_last"
email = "/data/personal/email"
attributes.rating = "/data/vatsim/rating/short"
attributes.subdivision = "/data/vatsim/subdivision/id"
"#;

const OIDC: &str = r#"
[[providers]]
kind = "oidc"
key = "corp"
label = "Sign in with Corp"
issuer = "https://idp.example.test/realms/corp"
client_id = "stepclimb"
client_secret = "corp-inline-secret"
"#;

fn env(insecure: bool) -> Env {
    let mut env: Env = [
        ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
        ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
        ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    if insecure {
        env.insert(
            "STEPCLIMB_ALLOW_INSECURE_PROVIDERS".to_owned(),
            "true".to_owned(),
        );
    }
    env
}

fn load_in(dir: &Dir, text: &str, insecure: bool) -> Result<Config, ConfigError> {
    let file = ConfigFile::parse(dir.0.join("stepclimb.toml"), text.to_owned())?;
    Config::load(&env(insecure), Some(&file))
}

fn load(text: &str) -> Result<Config, ConfigError> {
    let dir = Dir::new("load");
    load_in(&dir, text, false)
}

/// The refusal of `text`, as the message startup prints.
fn refused(text: &str) -> String {
    let dir = Dir::new("refused");
    let message = load_in(&dir, text, false)
        .map(|config| format!("accepted: {:?}", config.providers))
        .unwrap_or_else(|error| error.to_string());
    assert!(!message.contains(SECRET), "{message}");
    assert!(!message.contains("corp-inline-secret"), "{message}");
    message
}

/// `CANONICAL` with `line` (a `key = value` line or its key alone, to drop it) replaced.
fn canonical_with(line: &str, replacement: &str) -> String {
    let mut found = false;
    let text = CANONICAL
        .lines()
        .map(|l| {
            if l == line || l.split(" = ").next() == Some(line) {
                found = true;
                replacement.to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(found, "{line} is not a line of CANONICAL");
    text
}

#[test]
fn the_canonical_example_loads() {
    let config = load(CANONICAL).expect("loads");
    let [provider] = config.providers.as_slice() else {
        panic!("one provider: {:?}", config.providers);
    };
    assert_eq!(provider.entry, 1);
    assert_eq!(provider.key, "vatsim");
    assert_eq!(provider.kind, Kind::Oauth2);
    assert_eq!(provider.label, "Sign in with VATSIM");
    assert!(provider.enabled, "enabled defaults to true");
    assert_eq!(provider.org, None);
    assert_eq!(
        provider.secret().expose_secret(),
        SECRET,
        "read and trimmed"
    );
    let stored = provider
        .stored(UrlPolicy::new(false, false))
        .expect("valid under the strictest policy");
    assert_eq!(
        Value::Object(stored.clone()),
        json!({
            "authorize": "https://auth.vatsim.net/oauth/authorize",
            "token": "https://auth.vatsim.net/oauth/token",
            "userinfo": "https://auth.vatsim.net/api/user",
            "client_id": "stepclimb-test-client",
            "source": "config",
            "scopes": ["full_name", "vatsim_details", "email"],
            "pkce": true,
            "claims": {
                "subject": "/data/cid",
                "given_name": "/data/personal/name_first",
                "family_name": "/data/personal/name_last",
                "email": "/data/personal/email",
                "attributes": {
                    "rating": "/data/vatsim/rating/short",
                    "subdivision": "/data/vatsim/subdivision/id",
                },
            },
            "allow_local_passkeys": false,
            "trusted_for_email_matching": false,
            "label": "Sign in with VATSIM",
        })
    );
    let text = serde_json::to_string(&stored).unwrap();
    assert!(!text.contains(SECRET), "{text}");
    assert_eq!(
        Oauth2Config::from_stored(&stored)
            .expect("reads back")
            .sealed_secret(),
        None
    );
}

#[test]
fn an_oidc_entry_loads_with_its_defaults_and_an_inline_secret() {
    let config = load(OIDC).expect("loads");
    let [provider] = config.providers.as_slice() else {
        panic!("one provider");
    };
    assert_eq!(provider.kind, Kind::Oidc);
    assert_eq!(provider.secret().expose_secret(), "corp-inline-secret");
    let stored = provider
        .stored(UrlPolicy::new(false, false))
        .expect("valid");
    assert_eq!(stored["scopes"], json!(["openid", "email", "profile"]));
    assert_eq!(stored["source"], json!("config"));
    assert!(!stored.contains_key("client_secret"));
    assert!(OidcConfig::from_stored(&stored).is_ok());
}

/// T9: no `token` stops startup naming the key and the field.
#[test]
fn a_missing_token_is_refused_naming_the_key_and_the_field() {
    let message = refused(&canonical_with("token", ""));
    assert_eq!(
        message,
        format!(
            "{}: [[providers]] entry 1 (key \"vatsim\"): `token` is required",
            std::env::temp_dir()
                .join(format!(
                    "stepclimb-providers-{}-refused",
                    std::process::id()
                ))
                .join("stepclimb.toml")
                .display()
        )
    );
}

#[test]
fn every_kind_problem_is_reported_by_its_member() {
    let message = refused(
        &canonical_with("subject", "subject = \"data/cid\"")
            .replace("scopes = [\"full_name\"", "scopes = [\"full name\""),
    );
    assert!(
        message.contains("`claims.subject` is not a JSON Pointer"),
        "{message}"
    );
    assert!(
        message.contains("`scopes.0` is not an OAuth 2 scope token"),
        "{message}"
    );
}

#[test]
fn an_entry_without_a_key_is_named_by_its_position() {
    let text = format!("{OIDC}{}", canonical_with("key", ""));
    let message = refused(&text);
    assert!(
        message.ends_with("[[providers]] entry 2: `key` is required"),
        "{message}"
    );
}

#[test]
fn the_kind_must_be_oidc_or_oauth2() {
    assert!(refused(&canonical_with("kind", "")).contains("`kind` is required"));
    for kind in ["local", "saml", "OAuth2"] {
        let message = refused(&canonical_with("kind", &format!("kind = \"{kind}\"")));
        assert!(
            message.contains("`kind` must be \"oidc\" or \"oauth2\""),
            "{message}"
        );
    }
    let message = refused(&canonical_with("kind", "kind = 2"));
    assert!(message.contains("`kind` must be"), "{message}");
}

#[test]
fn the_key_follows_q5_and_cannot_be_local() {
    for key in ["Vatsim", "-vatsim", "vat_sim", "", &"k".repeat(33)] {
        let message = refused(&canonical_with("key", &format!("key = \"{key}\"")));
        assert!(
            message.contains("`key` must be 1 to 32 lowercase"),
            "{key:?}: {message}"
        );
    }
    let message = refused(&canonical_with("key", "key = \"local\""));
    assert!(message.contains("`key` cannot be \"local\""), "{message}");
    assert!(
        load(&canonical_with(
            "key",
            &format!("key = \"{}\"", "k".repeat(32))
        ))
        .is_ok()
    );
}

#[test]
fn the_label_is_required_short_and_printable() {
    assert!(refused(&canonical_with("label", "")).contains("`label` is required"));
    for label in [
        String::new(),
        "x".repeat(LABEL_MAX_CHARS + 1),
        "Sign in\\nwith VATSIM".to_owned(),
    ] {
        let message = refused(&canonical_with("label", &format!("label = \"{label}\"")));
        assert!(
            message.contains("`label` must be a string of 1 to 200"),
            "{message}"
        );
    }
    let longest = "é".repeat(LABEL_MAX_CHARS);
    let config = load(&canonical_with("label", &format!("label = \"{longest}\""))).expect("loads");
    assert_eq!(config.providers[0].label, longest);
}

#[test]
fn enabled_is_a_boolean() {
    let off = load(&format!("{CANONICAL}\n")).map(|c| c.providers[0].enabled);
    assert_eq!(off, Ok(true));
    let text = canonical_with("label", "label = \"VATSIM\"\nenabled = false");
    assert!(!load(&text).expect("loads").providers[0].enabled);
    let text = canonical_with("label", "label = \"VATSIM\"\nenabled = \"false\"");
    assert!(refused(&text).contains("`enabled` must be true or false"));
}

#[test]
fn org_is_a_slug_kept_for_startup() {
    let text = canonical_with("label", "label = \"VATSIM\"\norg = \"example\"");
    assert_eq!(
        load(&text).expect("loads").providers[0].org.as_deref(),
        Some("example")
    );
    for org in ["\"\"", "3"] {
        let text = canonical_with("label", &format!("label = \"VATSIM\"\norg = {org}"));
        assert!(refused(&text).contains("`org` must be an organization's slug"));
    }
}

#[test]
fn exactly_one_secret_source_is_required() {
    let message = refused(&canonical_with("client_secret_file", ""));
    assert!(
        message.contains("`client_secret` or `client_secret_file` is required"),
        "{message}"
    );
    let both = canonical_with(
        "client_secret_file",
        &format!("client_secret_file = \"vatsim_client_secret\"\nclient_secret = \"{SECRET}\""),
    );
    assert!(refused(&both).contains("set only one of `client_secret` and `client_secret_file`"));
    let inline = canonical_with("client_secret_file", "client_secret = 42");
    assert!(refused(&inline).contains("`client_secret` must be a string in quotes"));
    let empty = canonical_with("client_secret_file", "client_secret = \"\"");
    assert!(refused(&empty).contains("the client secret is empty"));
    let empty_file = canonical_with(
        "client_secret_file",
        "client_secret_file = \"empty_secret\"",
    );
    assert!(refused(&empty_file).contains("the client secret is empty"));
    let not_a_path = canonical_with("client_secret_file", "client_secret_file = \"\"");
    assert!(refused(&not_a_path).contains("`client_secret_file` must be a path in quotes"));
}

#[test]
fn an_unreadable_secret_file_is_refused_without_its_path() {
    let path = "/nonexistent/zq-secret-path";
    let text = canonical_with(
        "client_secret_file",
        &format!("client_secret_file = \"{path}\""),
    );
    let message = refused(&text);
    assert!(
        message
            .contains("`client_secret_file` names a file that could not be read: entity not found"),
        "{message}"
    );
    assert!(!message.contains("zq-secret-path"), "{message}");
}

#[test]
fn a_member_neither_kind_has_is_refused() {
    let text = canonical_with("label", "label = \"VATSIM\"\npreset = \"vatsim\"");
    assert!(refused(&text).contains("`preset` is not a member of this kind of provider"));
    let text = canonical_with("client_id", "client_id = 1979-05-27");
    assert!(refused(&text).contains("`client_id` has the wrong type"));
}

#[test]
fn admission_rules_cannot_be_declared_in_the_file() {
    let text = format!("{CANONICAL}\n[providers.admission_rules]\nrules = []\n");
    let message = refused(&text);
    assert!(
        message.contains("`admission_rules` cannot be declared in stepclimb.toml"),
        "{message}"
    );
}

#[test]
fn http_urls_need_the_insecure_switch() {
    let text = canonical_with("token", "token = \"http://127.0.0.1:9/oauth/token\"");
    let message = refused(&text);
    assert!(
        message.contains("`token` is refused by the outbound URL policy"),
        "{message}"
    );
    let dir = Dir::new("insecure");
    assert!(load_in(&dir, &text, true).is_ok());
}

#[test]
fn providers_must_be_an_array_of_tables() {
    let message = refused("providers = \"vatsim\"\n");
    assert!(
        message.contains("`providers`") && message.contains("line 1"),
        "{message}"
    );
    assert!(message.contains("must be an array of tables"), "{message}");
    assert!(!message.contains("\"vatsim\""), "{message}");
    let message = refused("providers = [\"vatsim\"]\n");
    assert!(
        message.contains("[[providers]] entry 1: is not a table"),
        "{message}"
    );
}

#[test]
fn a_key_used_twice_for_one_organization_is_refused() {
    let message = refused(&format!("{OIDC}{OIDC}"));
    assert!(
        message.contains(
            "[[providers]] entry 2 (key \"corp\"): `key` is also the key of entry 1 for the same organization"
        ),
        "{message}"
    );
    let other_org = OIDC.replace("kind = \"oidc\"", "kind = \"oidc\"\norg = \"other\"");
    let config = load(&format!("{OIDC}{other_org}")).expect("two organizations may share a key");
    assert_eq!(config.providers.len(), 2);
}

#[test]
fn no_file_means_no_providers() {
    let config = Config::load(&env(false), None).unwrap();
    assert!(config.providers.is_empty());
    assert_eq!(config.providers_or_none(), "(none)");
}

#[test]
fn no_output_shows_a_secret() {
    let disabled = OIDC.replace(
        "kind = \"oidc\"",
        "kind = \"oidc\"\nenabled = false\norg = \"example\"",
    );
    let config = load(&format!("{CANONICAL}\n{disabled}")).expect("loads");
    assert_eq!(
        config.providers_or_none(),
        "vatsim (oauth2), corp (oidc, org example, disabled)"
    );
    let shown = format!("{config:?} {:?}", config.providers);
    for secret in [SECRET, "corp-inline-secret", "vatsim_client_secret"] {
        assert!(!shown.contains(secret), "{secret}: {shown}");
    }
}

/// The member the core repository and the auth config types read is the same.
#[test]
fn core_and_auth_name_the_source_member_alike() {
    use stepclimb_auth::provider_config;
    assert_eq!(identity_providers::SOURCE, provider_config::SOURCE);
    assert_eq!(
        identity_providers::SOURCE_CONFIG,
        provider_config::SOURCE_CONFIG
    );
}
