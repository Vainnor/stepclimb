// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::encryption::DecryptError;

const POLICY: UrlPolicy = UrlPolicy::new(false, true);
const SECRET: &str = "test-client-secret-value";

fn ring() -> KeyRing {
    KeyRing::parse("k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").expect("a ring")
}

fn row() -> ProviderRow {
    ProviderRow {
        org_id: Uuid::from_u128(1),
        provider_id: Uuid::from_u128(2),
    }
}

fn minimal() -> Value {
    json!({
        "authorize": "https://idp.example.test/oauth/authorize",
        "token": "https://idp.example.test/oauth/token",
        "userinfo": "https://idp.example.test/api/user",
        "client_id": "stepclimb",
        "client_secret": SECRET,
        "claims": { "subject": "/data/id" },
    })
}

fn with(member: &str, value: Value) -> Value {
    let mut config = minimal();
    config[member] = value;
    config
}

fn without(member: &str) -> Value {
    let mut config = minimal();
    config.as_object_mut().expect("an object").remove(member);
    config
}

/// The problems of a refused config as `(pointer, code)` pairs.
fn problems(config: &Value) -> Vec<(String, &'static str)> {
    match validate(config, POLICY, None) {
        Ok(draft) => panic!("accepted: {draft:?}"),
        Err(ConfigErrors(problems)) => problems.into_iter().map(|p| (p.pointer, p.code)).collect(),
    }
}

fn one(pointer: &str, code: &'static str) -> Vec<(String, &'static str)> {
    vec![(pointer.to_owned(), code)]
}

fn sealed(config: &Value) -> Oauth2Config {
    validate(config, POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed")
}

#[test]
fn spec_t2_each_missing_member_or_bad_pointer_names_its_field() {
    for member in ["authorize", "token", "userinfo", "client_id"] {
        assert_eq!(
            problems(&without(member)),
            one(&format!("/{member}"), code::FIELD_REQUIRED),
            "{member}"
        );
    }
    assert_eq!(
        problems(&with("claims", json!({ "email": "/data/email" }))),
        one("/claims/subject", code::FIELD_REQUIRED)
    );
    assert_eq!(
        problems(&with(
            "claims",
            json!({ "subject": "/data/id", "email": "data/email" })
        )),
        one("/claims/email", code::POINTER_INVALID)
    );
}

#[test]
fn claims_are_required_and_an_object() {
    assert_eq!(
        problems(&without("claims")),
        one("/claims", code::FIELD_REQUIRED)
    );
    assert_eq!(
        problems(&with("claims", json!("/data/id"))),
        one("/claims", code::FIELD_TYPE)
    );
    for bad in [json!("data/id"), json!(""), json!("/a~2")] {
        assert_eq!(
            problems(&with("claims", json!({ "subject": bad }))),
            one("/claims/subject", code::POINTER_INVALID),
            "{bad}"
        );
    }
    assert_eq!(
        problems(&with("claims", json!({ "subject": 7 }))),
        one("/claims/subject", code::FIELD_TYPE)
    );
}

#[test]
fn every_endpoint_follows_the_outbound_url_policy() {
    for member in ["authorize", "token", "userinfo"] {
        for url in [
            "http://idp.example.test/x",
            "https://127.0.0.1/x",
            "https://[::1]/x",
            "https://169.254.169.254/x",
            "https://user:pass@idp.example.test/x",
        ] {
            assert_eq!(
                problems(&with(member, json!(url))),
                one(&format!("/{member}"), code::URL_REFUSED),
                "{member} {url}"
            );
        }
        // A multi-organization instance also refuses private addresses.
        let private = with(member, json!("https://10.0.0.5/x"));
        assert!(validate(&private, POLICY, None).is_ok(), "{member}");
        let multi = UrlPolicy::new(false, false);
        let Err(ConfigErrors(found)) = validate(&private, multi, None) else {
            panic!("{member}: accepted on a multi-organization instance");
        };
        assert_eq!(
            found,
            [ConfigProblem {
                pointer: format!("/{member}"),
                code: code::URL_REFUSED,
            }]
        );
    }
    // The insecure switch allows http and loopback, as tests and the mock provider need.
    let insecure = UrlPolicy::new(true, false);
    let local = json!({
        "authorize": "http://127.0.0.1:9000/oauth2/authorize",
        "token": "http://127.0.0.1:9000/oauth2/token",
        "userinfo": "http://127.0.0.1:9000/oauth2/userinfo",
        "client_id": "stepclimb",
        "client_secret": SECRET,
        "claims": { "subject": "/sub" },
    });
    assert!(validate(&local, insecure, None).is_ok());
    assert!(validate(&local, POLICY, None).is_err());
}

#[test]
fn an_endpoint_with_a_fragment_blank_or_other_scheme_is_invalid() {
    for member in ["authorize", "token", "userinfo"] {
        for url in [
            "https://idp.example.test/x#a",
            " https://idp.example.test/x",
            "https://idp.exa\tmple.test/x",
            "https://idp.example.test/x\n",
            "ftp://idp.example.test/x",
            "/oauth/authorize",
            "",
        ] {
            assert_eq!(
                problems(&with(member, json!(url))),
                one(&format!("/{member}"), code::URL_INVALID),
                "{member} {url:?}"
            );
        }
        assert_eq!(
            problems(&with(member, json!(["https://idp.example.test/x"]))),
            one(&format!("/{member}"), code::FIELD_TYPE),
            "{member}"
        );
    }
}

#[test]
fn an_endpoint_may_carry_a_query_and_is_kept_exactly_as_written() {
    let config = json!({
        "authorize": "https://IDP.example.test:443/oauth/authorize?tenant=a",
        "token": "https://idp.example.test/oauth/token?tenant=a&x=",
        "userinfo": "https://idp.example.test/api/user/",
        "client_id": "stepclimb",
        "client_secret": SECRET,
        "claims": { "subject": "/data/id" },
    });
    let settings = validate(&config, POLICY, None).expect("valid").settings;
    assert_eq!(
        settings.authorize,
        "https://IDP.example.test:443/oauth/authorize?tenant=a"
    );
    assert_eq!(
        settings.token,
        "https://idp.example.test/oauth/token?tenant=a&x="
    );
    assert_eq!(settings.userinfo, "https://idp.example.test/api/user/");
}

#[test]
fn defaults_fill_scopes_pkce_passkeys_and_leave_profile_fields_unmapped() {
    let settings = validate(&minimal(), POLICY, None).expect("valid").settings;
    assert!(settings.scopes.is_empty());
    assert!(settings.pkce);
    assert!(!settings.allow_local_passkeys);
    assert_eq!(settings.claims.subject.as_str(), "/data/id");
    assert_eq!(settings.claims.profile, ProfileMapping::default());
}

/// The canonical example of spec 06 B11, as JSON.
fn vatsim_shaped() -> Value {
    json!({
        "authorize": "https://auth.example.test/oauth/authorize",
        "token": "https://auth.example.test/oauth/token",
        "userinfo": "https://auth.example.test/api/user",
        "scopes": ["full_name", "vatsim_details", "email"],
        "client_id": "stepclimb",
        "client_secret": SECRET,
        "pkce": false,
        "allow_local_passkeys": true,
        "claims": {
            "subject": "/data/cid",
            "given_name": "/data/personal/name_first",
            "family_name": "/data/personal/name_last",
            "email": "/data/personal/email",
            "email_verified": "/data/personal/email_verified",
            "groups": "/data/groups",
            "attributes": {
                "rating": "/data/vatsim/rating/short",
                "subdivision": "/data/vatsim/subdivision/id",
            },
        },
    })
}

#[test]
fn a_full_config_is_read_member_by_member() {
    let settings = validate(&vatsim_shaped(), POLICY, None)
        .expect("valid")
        .settings;
    assert_eq!(
        settings.authorize,
        "https://auth.example.test/oauth/authorize"
    );
    assert_eq!(settings.token, "https://auth.example.test/oauth/token");
    assert_eq!(settings.userinfo, "https://auth.example.test/api/user");
    assert_eq!(settings.client_id, "stepclimb");
    assert_eq!(settings.scopes, ["full_name", "vatsim_details", "email"]);
    assert!(!settings.pkce);
    assert!(settings.allow_local_passkeys);
    let claims = &settings.claims;
    assert_eq!(claims.subject.as_str(), "/data/cid");
    let pointer = |p: &Option<JsonPointer>| p.as_ref().map(|p| p.as_str().to_owned());
    let profile = &claims.profile;
    assert_eq!(
        pointer(&profile.given_name).as_deref(),
        Some("/data/personal/name_first")
    );
    assert_eq!(
        pointer(&profile.family_name).as_deref(),
        Some("/data/personal/name_last")
    );
    assert_eq!(
        pointer(&profile.email).as_deref(),
        Some("/data/personal/email")
    );
    assert_eq!(
        pointer(&profile.email_verified).as_deref(),
        Some("/data/personal/email_verified")
    );
    assert_eq!(pointer(&profile.groups).as_deref(), Some("/data/groups"));
    assert_eq!(
        profile.attributes["rating"].as_str(),
        "/data/vatsim/rating/short"
    );
    assert_eq!(
        profile.attributes["subdivision"].as_str(),
        "/data/vatsim/subdivision/id"
    );
    assert_eq!(profile.attributes.len(), 2);
}

#[test]
fn a_validated_config_maps_a_userinfo_body() {
    let claims = validate(&vatsim_shaped(), POLICY, None)
        .expect("valid")
        .settings
        .claims;
    let body = json!({
        "data": {
            "cid": 1000001,
            "personal": {
                "name_first": "Test",
                "name_last": "Student",
                "email": "student@example.test",
                "email_verified": true,
            },
            "vatsim": { "rating": { "short": "S1" }, "subdivision": { "id": "USA" } },
        },
    });
    let mapped = claims.map(&body).expect("a subject");
    assert_eq!(mapped.subject, "1000001");
    assert_eq!(mapped.profile.given_name.as_deref(), Some("Test"));
    assert_eq!(mapped.profile.family_name.as_deref(), Some("Student"));
    assert_eq!(
        mapped.profile.email.as_deref(),
        Some("student@example.test")
    );
    assert_eq!(mapped.profile.email_verified, Some(true));
    assert_eq!(mapped.profile.groups, None);
    assert_eq!(
        Value::Object(mapped.profile.attributes),
        json!({ "rating": "S1", "subdivision": "USA" })
    );
    let mut no_cid = body;
    no_cid["data"]["cid"] = json!(false);
    assert_eq!(claims.map(&no_cid), Err(SubjectMissing));
}

#[test]
fn flags_and_scopes_must_have_their_types() {
    assert_eq!(
        problems(&with("pkce", json!("true"))),
        one("/pkce", code::FIELD_TYPE)
    );
    assert_eq!(
        problems(&with("allow_local_passkeys", json!(1))),
        one("/allow_local_passkeys", code::FIELD_TYPE)
    );
    assert_eq!(
        problems(&with("scopes", json!("email"))),
        one("/scopes", code::FIELD_TYPE)
    );
    // An oauth2 provider has no `openid` rule.
    let scopes = validate(&with("scopes", json!(["email"])), POLICY, None)
        .expect("valid")
        .settings
        .scopes;
    assert_eq!(scopes, ["email"]);
}

#[test]
fn every_problem_is_listed_at_once() {
    let config = json!({
        "authorize": "nope",
        "token": "https://idp.example.test/t#frag",
        "client_id": "",
        "scopes": ["email", "two words", 3],
        "pkce": "yes",
        "claims": {
            "subjcet": "/id",
            "email": "email",
            "groups": 1,
            "attributes": { "": "/x", "d": "d" },
        },
        "allow_local_passkeys": "no",
        "issuer": "https://idp.example.test/",
    });
    let mut found = problems(&config);
    found.sort();
    let mut expected = vec![
        ("/issuer".to_owned(), code::FIELD_UNKNOWN),
        ("/authorize".to_owned(), code::URL_INVALID),
        ("/token".to_owned(), code::URL_INVALID),
        ("/userinfo".to_owned(), code::FIELD_REQUIRED),
        ("/client_id".to_owned(), code::FIELD_EMPTY),
        ("/scopes/1".to_owned(), code::SCOPE_INVALID),
        ("/scopes/2".to_owned(), code::FIELD_TYPE),
        ("/pkce".to_owned(), code::FIELD_TYPE),
        ("/claims/subjcet".to_owned(), code::FIELD_UNKNOWN),
        ("/claims/subject".to_owned(), code::FIELD_REQUIRED),
        ("/claims/email".to_owned(), code::POINTER_INVALID),
        ("/claims/groups".to_owned(), code::FIELD_TYPE),
        ("/claims/attributes/".to_owned(), code::FIELD_EMPTY),
        ("/claims/attributes/d".to_owned(), code::POINTER_INVALID),
        ("/allow_local_passkeys".to_owned(), code::FIELD_TYPE),
        ("/client_secret".to_owned(), code::FIELD_REQUIRED),
    ];
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn nul_in_a_stored_string_is_refused() {
    assert_eq!(
        problems(&with("client_id", json!("a\u{0}b"))),
        one("/client_id", code::TEXT_CONTAINS_NUL)
    );
    assert_eq!(
        problems(&with(
            "claims",
            json!({ "subject": "/id", "attributes": { "a\u{0}": "/a" } })
        )),
        one("/claims/attributes/a\u{0}", code::TEXT_CONTAINS_NUL)
    );
}

#[test]
fn a_config_that_is_not_an_object_is_refused() {
    assert_eq!(problems(&json!(["authorize"])), one("", code::FIELD_TYPE));
    assert_eq!(problems(&json!(null)), one("", code::FIELD_TYPE));
}

#[test]
fn the_secret_is_required_unless_one_is_stored() {
    assert_eq!(
        problems(&without("client_secret")),
        one("/client_secret", code::FIELD_REQUIRED)
    );
    assert_eq!(
        problems(&with("client_secret", json!(""))),
        one("/client_secret", code::FIELD_EMPTY)
    );
    assert_eq!(
        problems(&with("client_secret", json!(42))),
        one("/client_secret", code::FIELD_TYPE)
    );
    let stored = sealed(&minimal());
    let kept = validate(&without("client_secret"), POLICY, stored.sealed_secret())
        .expect("valid with the stored secret")
        .seal(&ring(), row())
        .expect("sealed");
    assert_eq!(
        kept.client_secret(&ring(), row(), &ConfigSecrets::default())
            .expect("opens")
            .expose_secret(),
        SECRET
    );
    let replaced = validate(
        &with("client_secret", json!("a-new-secret")),
        POLICY,
        stored.sealed_secret(),
    )
    .expect("valid")
    .seal(&ring(), row())
    .expect("sealed");
    assert_eq!(
        replaced
            .client_secret(&ring(), row(), &ConfigSecrets::default())
            .expect("opens")
            .expose_secret(),
        "a-new-secret"
    );
}

#[test]
fn the_stored_config_holds_the_secret_only_sealed_and_reads_back() {
    for config in [minimal(), vatsim_shaped()] {
        let config = sealed(&config);
        let stored = config.to_json();
        let text = serde_json::to_string(&stored).expect("JSON");
        assert!(!text.contains(SECRET), "{text}");
        assert!(stored["client_secret"]["sealed"].is_string(), "{text}");
        let read = Oauth2Config::from_stored(&stored).expect("reads back");
        assert_eq!(read, config);
        assert_eq!(
            read.client_secret(&ring(), row(), &ConfigSecrets::default())
                .expect("opens")
                .expose_secret(),
            SECRET
        );
    }
}

#[test]
fn a_sealed_secret_opens_only_for_its_own_row() {
    let config = sealed(&minimal());
    let other_provider = ProviderRow {
        provider_id: Uuid::from_u128(3),
        ..row()
    };
    let other_org = ProviderRow {
        org_id: Uuid::from_u128(9),
        ..row()
    };
    for wrong in [other_provider, other_org] {
        assert_eq!(
            config
                .client_secret(&ring(), wrong, &ConfigSecrets::default())
                .err(),
            Some(SecretError::Decrypt(DecryptError::Inauthentic)),
            "{wrong:?}"
        );
    }
}

#[test]
fn nothing_prints_the_secret() {
    let draft = validate(&minimal(), POLICY, None).expect("valid");
    let config = draft.clone().seal(&ring(), row()).expect("sealed");
    let opened = config
        .client_secret(&ring(), row(), &ConfigSecrets::default())
        .expect("opens");
    let shown = format!("{draft:?} {config:?} {opened:?}");
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("sealed\""), "{shown}");
}

#[test]
fn a_stored_config_with_extra_members_reads_and_a_broken_one_fails() {
    let config = sealed(&minimal());
    let mut stored = config.to_json();
    stored.insert("label".into(), json!("Sign in with Example"));
    assert_eq!(Oauth2Config::from_stored(&stored), Ok(config.clone()));

    let mut plain_secret = config.to_json();
    plain_secret.insert("client_secret".into(), json!(SECRET));
    assert_eq!(
        Oauth2Config::from_stored(&plain_secret),
        Err(StoredConfigError::Invalid {
            pointer: "/client_secret".into(),
            code: code::FIELD_TYPE,
        })
    );
    let mut no_token = config.to_json();
    no_token.remove("token");
    assert_eq!(
        Oauth2Config::from_stored(&no_token),
        Err(StoredConfigError::Invalid {
            pointer: "/token".into(),
            code: code::FIELD_REQUIRED,
        })
    );
}

#[test]
fn a_stored_config_is_read_without_the_url_policy() {
    // A provider saved under the insecure switch still reads; the client refuses its URLs at
    // connect time once the switch is off.
    let insecure = UrlPolicy::new(true, false);
    let config = validate(
        &with("token", json!("http://127.0.0.1:9000/oauth2/token")),
        insecure,
        None,
    )
    .expect("valid")
    .seal(&ring(), row())
    .expect("sealed");
    assert!(Oauth2Config::from_stored(&config.to_json()).is_ok());
}

#[test]
fn a_single_unknown_member_refuses_the_config() {
    assert_eq!(
        problems(&with("pkc", json!(false))),
        one("/pkc", code::FIELD_UNKNOWN)
    );
    assert_eq!(
        problems(&with(
            "claims",
            json!({ "subject": "/id", "given_nmae": "/n" })
        )),
        one("/claims/given_nmae", code::FIELD_UNKNOWN)
    );
}

/// The context is written out here, not taken from the code under test, so a change to it
/// (which would leave every stored secret unreadable) turns this red.
#[test]
fn the_secret_is_sealed_to_the_literal_row_context() {
    use base64::Engine as _;
    let draft: Oauth2ConfigDraft = validate(&minimal(), POLICY, None).expect("valid");
    let stored = draft.seal(&ring(), row()).expect("sealed").to_json();
    let text = stored["client_secret"]["sealed"]
        .as_str()
        .expect("a string");
    let sealed = base64::engine::general_purpose::STANDARD
        .decode(text)
        .expect("standard base64");
    let org = Uuid::from_u128(1);
    let pid = Uuid::from_u128(2);
    let context: [&[u8]; 4] = [
        org.as_bytes(),
        b"identity_providers",
        pid.as_bytes(),
        b"client_secret",
    ];
    assert_eq!(
        ring().decrypt(&sealed, &context).expect("opens"),
        SECRET.as_bytes()
    );
    let swapped: [&[u8]; 4] = [
        pid.as_bytes(),
        b"identity_providers",
        org.as_bytes(),
        b"client_secret",
    ];
    assert!(ring().decrypt(&sealed, &swapped).is_err());
}

#[test]
fn an_authorize_query_may_not_set_a_parameter_the_request_sets() {
    for name in [
        "response_type",
        "client_id",
        "redirect_uri",
        "scope",
        "state",
        "code_challenge",
        "code_challenge_method",
        "nonce",
    ] {
        let url = format!("https://idp.example.test/oauth/authorize?tenant=a&{name}=x");
        assert_eq!(
            problems(&with("authorize", json!(url))),
            one("/authorize", code::URL_PARAMETER_RESERVED),
            "{name}"
        );
    }
    // Names are compared decoded; an empty value still sets the parameter.
    for url in [
        "https://idp.example.test/oauth/authorize?%73tate=x",
        "https://idp.example.test/oauth/authorize?scope",
    ] {
        assert_eq!(
            problems(&with("authorize", json!(url))),
            one("/authorize", code::URL_PARAMETER_RESERVED),
            "{url}"
        );
    }
    // Other parameters, and the same names on the token and userinfo URLs, are allowed.
    for (member, url) in [
        (
            "authorize",
            "https://idp.example.test/oauth/authorize?prompt=login&states=1",
        ),
        ("token", "https://idp.example.test/oauth/token?client_id=x"),
        ("userinfo", "https://idp.example.test/api/user?scope=x"),
    ] {
        assert!(
            validate(&with(member, json!(url)), POLICY, None).is_ok(),
            "{url}"
        );
    }
}

/// Spec 06 B10: with an `email_verified` pointer mapped, the email is kept only when the value
/// there is exactly `true`; without one, the email is kept as sent.
#[test]
fn b10_an_email_the_provider_does_not_vouch_for_is_dropped() {
    let mut config = minimal();
    config["claims"] = json!({
        "subject": "/id",
        "email": "/email",
        "email_verified": "/verified",
    });
    let claims = validate(&config, POLICY, None)
        .expect("valid")
        .settings
        .claims;
    let email = |verified: Option<Value>| {
        let mut body = json!({ "id": "1", "email": "student@example.test" });
        if let Some(verified) = verified {
            body["verified"] = verified;
        }
        claims.map(&body).expect("a subject").profile.email
    };
    assert_eq!(
        email(Some(json!(true))).as_deref(),
        Some("student@example.test")
    );
    for refused in [
        None,
        Some(json!(false)),
        Some(json!("true")),
        Some(json!(1)),
        Some(Value::Null),
    ] {
        assert_eq!(email(refused.clone()), None, "{refused:?}");
    }

    let unmapped = validate(&minimal(), POLICY, None)
        .expect("valid")
        .settings
        .claims;
    let mut with_email = unmapped.clone();
    with_email.profile.email = Some(crate::json_pointer::JsonPointer::parse("/email").unwrap());
    let body = json!({ "data": { "id": "1" }, "email": "student@example.test" });
    assert_eq!(
        with_email.map(&body).unwrap().profile.email.as_deref(),
        Some("student@example.test"),
        "no email_verified pointer: the email is kept"
    );
}

/// A provider `stepclimb.toml` declares stores `source: "config"` and no secret
/// (`05-oidc.md` Q2 (c)); its secret comes only from the server's [`ConfigSecrets`].
#[test]
fn a_config_file_provider_stores_no_secret_and_reads_it_from_the_config_secrets() {
    let settings = validate(&minimal(), POLICY, None).expect("valid").settings;
    let config = Oauth2Config::from_config_file(settings);
    let stored = config.to_json();
    assert_eq!(stored["source"], json!("config"));
    assert!(!stored.contains_key("client_secret"), "{stored:?}");
    let text = serde_json::to_string(&stored).expect("json");
    assert!(!text.contains(SECRET), "{text}");

    let read = Oauth2Config::from_stored(&stored).expect("reads back");
    assert_eq!(read, config);
    assert_eq!(read.sealed_secret(), None);
    assert_eq!(
        read.client_secret(&ring(), row(), &ConfigSecrets::default())
            .err(),
        Some(SecretError::NotInConfig)
    );
    let mut secrets = ConfigSecrets::default();
    secrets.insert(
        row(),
        ClientSecret::from_config_file(SECRET.to_owned()).expect("not empty"),
    );
    let opened = read.client_secret(&ring(), row(), &secrets).expect("held");
    assert_eq!(opened.expose_secret(), SECRET);
    let other_org = ProviderRow {
        org_id: Uuid::from_u128(9),
        ..row()
    };
    assert_eq!(
        read.client_secret(&ring(), other_org, &secrets).err(),
        Some(SecretError::NotInConfig)
    );
    assert!(!format!("{secrets:?}").contains(SECRET));
    assert!(ClientSecret::from_config_file(String::new()).is_none());
}

#[test]
fn a_stored_source_other_than_config_or_a_config_row_with_a_secret_fails_closed() {
    let settings = validate(&minimal(), POLICY, None).expect("valid").settings;
    let mut with_secret = Oauth2Config::from_config_file(settings).to_json();
    with_secret.insert("client_secret".into(), json!({ "sealed": "AAAA" }));
    assert_eq!(
        Oauth2Config::from_stored(&with_secret),
        Err(StoredConfigError::Invalid {
            pointer: "/client_secret".into(),
            code: code::FIELD_UNKNOWN,
        })
    );
    let sealed = validate(&minimal(), POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed");
    assert!(sealed.sealed_secret().is_some());
    for source in [json!("api"), json!(true), json!("Config")] {
        let mut stored = sealed.to_json();
        stored.insert("source".into(), source.clone());
        assert_eq!(
            Oauth2Config::from_stored(&stored),
            Err(StoredConfigError::Invalid {
                pointer: "/source".into(),
                code: code::FIELD_TYPE,
            }),
            "{source}"
        );
    }
}

#[test]
fn an_admin_config_cannot_claim_the_config_file_source() {
    assert_eq!(
        problems(&with("source", json!("config"))),
        one("/source", code::FIELD_UNKNOWN)
    );
}
