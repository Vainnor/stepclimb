// SPDX-License-Identifier: AGPL-3.0-or-later
use serde_json::json;
use stepclimb_auth::provider_config::TRUSTED_FOR_EMAIL_MATCHING;

use super::*;

fn new_provider(body: Value) -> Result<NewProvider, serde_json::Error> {
    serde_json::from_value(body)
}

fn patch(body: Value) -> Result<ProviderPatch, serde_json::Error> {
    serde_json::from_value(body)
}

#[test]
fn a_new_provider_is_enabled_unless_the_body_says_otherwise() {
    let body = json!({ "key": "sso", "kind": "oidc", "label": "SSO", "config": {} });
    assert!(new_provider(body.clone()).expect("parses").enabled);
    let mut off = body;
    off["enabled"] = json!(false);
    assert!(!new_provider(off).expect("parses").enabled);
}

#[test]
fn bodies_refuse_unknown_members_nulls_and_non_object_configs() {
    let base = json!({ "key": "sso", "kind": "oidc", "label": "SSO", "config": {} });
    for (member, value) in [
        ("secret", json!("x")),
        ("preset", json!(null)),
        ("config", json!([])),
        ("enabled", json!("yes")),
    ] {
        let mut body = base.clone();
        body[member] = value;
        assert!(new_provider(body).is_err(), "{member}");
    }
    for body in [
        json!({ "label": null }),
        json!({ "enabled": null }),
        json!({ "config": null }),
        json!({ "kind": "oauth2" }),
        json!({ "key": "other" }),
    ] {
        assert!(patch(body.clone()).is_err(), "{body}");
    }
    assert_eq!(patch(json!({})).expect("parses"), ProviderPatch::default());
}

#[test]
fn only_the_two_outside_kinds_and_known_presets_are_accepted() {
    let body = |kind: Option<&str>, preset: Option<&str>| NewProvider {
        key: Some("sso".into()),
        kind: kind.map(str::to_owned),
        preset: preset.map(str::to_owned),
        label: Some("SSO".into()),
        enabled: true,
        config: Map::new(),
    };
    let kind =
        |kind, preset| new_kind(&body(kind, preset)).map(|(kind, p)| (kind, p.map(|p| p.name)));
    assert_eq!(kind(Some("oidc"), None).unwrap(), (Kind::Oidc, None));
    assert_eq!(kind(Some("oauth2"), None).unwrap(), (Kind::Oauth2, None));
    for name in [Some("local"), Some("saml"), Some("OIDC"), Some(""), None] {
        let refused = kind(name, None).unwrap_err().problem();
        assert_eq!(
            (refused.status, refused.code.as_str()),
            (422, "provider.kind_unsupported"),
            "{name:?}"
        );
        assert_eq!(refused.errors[0].field, "/kind");
    }
    let unknown = kind(Some("oauth2"), Some("no-such-preset"))
        .unwrap_err()
        .problem();
    assert_eq!(
        (unknown.status, unknown.code.as_str()),
        (422, "provider.preset_unknown")
    );
    assert_eq!(unknown.errors[0].field, "/preset");
}

/// With the `vatsim` preset compiled in, its provider is `oauth2`, whether the body leaves the
/// kind out or names it; another kind is refused at `/kind`.
#[cfg(feature = "vatsim")]
#[test]
fn a_preset_gives_its_kind() {
    let body = |kind: Option<&str>| NewProvider {
        key: None,
        kind: kind.map(str::to_owned),
        preset: Some("vatsim".into()),
        label: None,
        enabled: true,
        config: Map::new(),
    };
    for named in [None, Some("oauth2")] {
        let (kind, preset) = new_kind(&body(named)).unwrap();
        assert_eq!(
            (kind, preset.map(|p| p.name)),
            (Kind::Oauth2, Some("vatsim"))
        );
    }
    let refused = new_kind(&body(Some("oidc"))).unwrap_err().problem();
    assert_eq!(refused.code, "provider.kind_unsupported");
    assert_eq!(refused.errors[0].field, "/kind");
}

#[test]
fn keys_follow_q5_and_are_never_local_or_a_dot_segment() {
    for key in ["sso", "corp-sso", "0", &"k".repeat(32)] {
        assert!(check_key(key).is_ok(), "{key:?}");
    }
    for key in ["local", ".", "..", "a/b", "Sso", "", "-x", &"k".repeat(33)] {
        let refused = check_key(key).unwrap_err().problem();
        assert_eq!(refused.code, "request.invalid", "{key:?}");
        assert_eq!(refused.errors[0].field, "/key");
        assert_eq!(refused.errors[0].code, "provider.key_invalid");
    }
}

#[test]
fn config_problems_point_under_config() {
    let problems = ConfigErrors(vec![
        stepclimb_auth::provider_config::ConfigProblem {
            pointer: "/claims/given_name".into(),
            code: "provider.pointer_invalid",
        };
        MAX_ERRORS + 5
    ]);
    let answer = config_invalid(problems).problem();
    assert_eq!(
        (answer.status, answer.code.as_str()),
        (422, "provider.config_invalid")
    );
    assert_eq!(answer.errors.len(), MAX_ERRORS);
    assert_eq!(answer.errors[0].field, "/config/claims/given_name");
}

#[test]
fn repository_refusals_map_to_their_status_and_code() {
    let cases = [
        (repo::Error::NotFound, 404, "provider.not_found"),
        (repo::Error::KeyTaken, 409, "provider.key_taken"),
        (repo::Error::InUse, 409, "provider.in_use"),
        (repo::Error::KeyInvalid, 422, "request.invalid"),
        (repo::Error::LabelInvalid, 422, "request.invalid"),
        (
            repo::Error::KindUnsupported,
            422,
            "provider.kind_unsupported",
        ),
        (repo::Error::SourceMember, 500, "internal.error"),
    ];
    for (error, status, code) in cases {
        let answer = provider_error(error).problem();
        assert_eq!((answer.status, answer.code.as_str()), (status, code));
    }
}

fn row(config: Value) -> IdentityProvider {
    let Value::Object(config) = config else {
        panic!("an object")
    };
    IdentityProvider {
        id: Uuid::nil(),
        key: "sso".into(),
        kind: Kind::Oidc,
        config,
        enabled: true,
        created_at: std::time::SystemTime::UNIX_EPOCH,
        updated_at: std::time::SystemTime::UNIX_EPOCH,
    }
}

#[test]
fn an_answer_never_holds_the_secret_and_says_whether_one_is_set() {
    let public_url = WebOrigin::parse("https://lms.example.org").unwrap();
    let sealed = row(json!({
        "issuer": "https://idp.example.test",
        "client_secret": { "sealed": "c2VhbGVkLXNlY3JldA==" },
        "label": "SSO",
        "admission_rules": [],
    }));
    let answer = Provider::of(&sealed, &public_url).unwrap();
    let text = serde_json::to_string(&answer).unwrap();
    assert!(!text.contains("c2VhbGVkLXNlY3JldA"), "{text}");
    assert!(!text.contains("admission_rules"), "{text}");
    assert!(answer.client_secret_set);
    assert_eq!(answer.source, ProviderSource::Api);
    assert_eq!(answer.label, "SSO");
    assert_eq!(
        answer.redirect_uri,
        "https://lms.example.org/auth/sso/callback"
    );

    let file =
        row(json!({ "issuer": "https://idp.example.test", "label": "SSO", "source": "config" }));
    let answer = Provider::of(&file, &public_url).unwrap();
    assert_eq!(answer.source, ProviderSource::Config);
    assert!(answer.client_secret_set, "the file holds the secret");
    assert!(!answer.config.contains_key("source"));

    let unlabeled = row(json!({ "issuer": "https://idp.example.test" }));
    assert_eq!(
        Provider::of(&unlabeled, &public_url).unwrap_err().status(),
        500
    );
}

#[test]
fn a_provider_is_trusted_for_email_matching_only_with_exactly_true() {
    for (value, expected) in [
        (json!(true), true),
        (json!(false), false),
        (json!("true"), false),
        (json!(1), false),
    ] {
        let config = row(json!({ TRUSTED_FOR_EMAIL_MATCHING: value })).config;
        assert_eq!(guard::trusted(&config), expected, "{value}");
    }
    assert!(!guard::trusted(&Map::new()));
}

#[test]
fn debug_names_config_members_but_never_the_secret() {
    let secret = "a-secret-only-this-test-knows";
    let body = json!({
        "key": "sso",
        "kind": "oidc",
        "label": "SSO",
        "config": { "issuer": "https://idp.example.test", "client_secret": secret },
    });
    let created = format!("{:?}", new_provider(body).expect("parses"));
    let changed = format!(
        "{:?}",
        patch(json!({ "config": { "client_secret": secret } })).expect("parses")
    );
    for debug in [created, changed] {
        assert!(debug.contains("client_secret"), "{debug}");
        assert!(!debug.contains(secret), "{debug}");
    }
}
