// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::json;
use uuid::Uuid;

use super::*;

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
        "issuer": "https://idp.example.test/realms/acme",
        "client_id": "stepclimb",
        "client_secret": SECRET,
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

#[test]
fn spec_t1_each_bad_config_names_its_field() {
    assert_eq!(
        problems(&without("issuer")),
        one("/issuer", code::FIELD_REQUIRED)
    );
    assert_eq!(
        problems(&with("issuer", json!("not a url"))),
        one("/issuer", code::URL_INVALID)
    );
    assert_eq!(
        problems(&with("scopes", json!(["email", "profile"]))),
        one("/scopes", code::SCOPES_MISSING_OPENID)
    );
    assert_eq!(
        problems(&with("claims", json!({ "given_name": "given_name" }))),
        one("/claims/given_name", code::POINTER_INVALID)
    );
    assert_eq!(
        problems(&without("client_id")),
        one("/client_id", code::FIELD_REQUIRED)
    );
}

#[test]
fn spec_t16_an_issuer_the_url_policy_refuses_is_refused_on_save() {
    for issuer in [
        "http://idp.example.test/",
        "https://127.0.0.1/realms/a",
        "https://[::1]/",
        "https://169.254.169.254/",
        "https://user:pass@idp.example.test/",
    ] {
        assert_eq!(
            problems(&with("issuer", json!(issuer))),
            one("/issuer", code::URL_REFUSED),
            "{issuer}"
        );
    }
    // A multi-organization instance also refuses private addresses.
    let multi = UrlPolicy::new(false, false);
    let private = with("issuer", json!("https://10.0.0.5/"));
    assert!(validate(&private, POLICY, None).is_ok());
    let Err(ConfigErrors(found)) = validate(&private, multi, None) else {
        panic!("accepted on a multi-organization instance");
    };
    assert_eq!(found[0].code, code::URL_REFUSED);
    // The insecure switch allows http and loopback, as tests and the development stack need.
    let insecure = UrlPolicy::new(true, false);
    let local = with("issuer", json!("http://127.0.0.1:9000/realms/a"));
    assert!(validate(&local, insecure, None).is_ok());
}

#[test]
fn an_issuer_is_kept_exactly_as_written() {
    for issuer in [
        "https://accounts.example.test",
        "https://idp.example.test/realms/acme/",
        "https://IDP.example.test:443/x",
    ] {
        let draft = validate(&with("issuer", json!(issuer)), POLICY, None).expect("valid");
        assert_eq!(draft.settings.issuer, issuer);
    }
}

#[test]
fn an_issuer_with_a_query_fragment_blank_or_other_scheme_is_invalid() {
    for issuer in [
        "https://idp.example.test/?tenant=a",
        "https://idp.example.test/#a",
        " https://idp.example.test/",
        "https://idp.exa\tmple.test/",
        "ftp://idp.example.test/",
        "",
    ] {
        assert_eq!(
            problems(&with("issuer", json!(issuer))),
            one("/issuer", code::URL_INVALID),
            "{issuer:?}"
        );
    }
    assert_eq!(
        problems(&with("issuer", json!(42))),
        one("/issuer", code::FIELD_TYPE)
    );
}

#[test]
fn defaults_fill_scopes_claims_and_userinfo() {
    let draft = validate(&minimal(), POLICY, None).expect("valid");
    let settings = draft.settings;
    assert_eq!(settings.scopes, ["openid", "email", "profile"]);
    assert_eq!(settings.claims.given_name.as_str(), "/given_name");
    assert_eq!(settings.claims.family_name.as_str(), "/family_name");
    assert_eq!(settings.claims.email.as_str(), "/email");
    assert_eq!(settings.claims.email_verified.as_str(), "/email_verified");
    assert_eq!(settings.claims.groups, None);
    assert!(settings.claims.attributes.is_empty());
    assert!(settings.userinfo);
}

#[test]
fn a_full_config_is_read_member_by_member() {
    let config = json!({
        "issuer": "https://idp.example.test/realms/acme",
        "client_id": "stepclimb",
        "client_secret": SECRET,
        "scopes": ["openid", "groups"],
        "claims": {
            "given_name": "/name/first",
            "groups": "/groups",
            "attributes": { "department": "/department", "a/b": "/x~1y" },
        },
        "userinfo": false,
    });
    let settings = validate(&config, POLICY, None).expect("valid").settings;
    assert_eq!(settings.scopes, ["openid", "groups"]);
    assert_eq!(settings.claims.given_name.as_str(), "/name/first");
    assert_eq!(settings.claims.family_name.as_str(), "/family_name");
    assert_eq!(
        settings.claims.groups.as_ref().map(JsonPointer::as_str),
        Some("/groups")
    );
    assert_eq!(
        settings.claims.attributes["department"].as_str(),
        "/department"
    );
    assert_eq!(settings.claims.attributes["a/b"].as_str(), "/x~1y");
    assert!(!settings.userinfo);
}

#[test]
fn every_problem_is_listed_at_once() {
    let config = json!({
        "issuer": "nope",
        "client_id": "",
        "scopes": ["openid", "two words", 3],
        "claims": { "subject": "/sub", "email": "email", "attributes": { "": "/x", "d": "d" } },
        "userinfo": "yes",
        "isuer": "https://idp.example.test/",
    });
    let mut found = problems(&config);
    found.sort();
    let mut expected = vec![
        ("/isuer".to_owned(), code::FIELD_UNKNOWN),
        ("/issuer".to_owned(), code::URL_INVALID),
        ("/client_id".to_owned(), code::FIELD_EMPTY),
        ("/scopes/1".to_owned(), code::SCOPE_INVALID),
        ("/scopes/2".to_owned(), code::FIELD_TYPE),
        ("/claims/subject".to_owned(), code::FIELD_UNKNOWN),
        ("/claims/email".to_owned(), code::POINTER_INVALID),
        ("/claims/attributes/".to_owned(), code::FIELD_EMPTY),
        ("/claims/attributes/d".to_owned(), code::POINTER_INVALID),
        ("/userinfo".to_owned(), code::FIELD_TYPE),
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
        problems(&with("claims", json!({ "attributes": { "a\u{0}": "/a" } }))),
        one("/claims/attributes/a\u{0}", code::TEXT_CONTAINS_NUL)
    );
}

#[test]
fn a_config_that_is_not_an_object_is_refused() {
    assert_eq!(problems(&json!(["issuer"])), one("", code::FIELD_TYPE));
    assert_eq!(
        problems(&with("claims", json!("/email"))),
        one("/claims", code::FIELD_TYPE)
    );
    assert_eq!(
        problems(&with("scopes", json!("openid"))),
        one("/scopes", code::FIELD_TYPE)
    );
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
    let stored = validate(&minimal(), POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed");
    let change = validate(
        &without("client_secret"),
        POLICY,
        Some(stored.sealed_secret()),
    )
    .expect("valid with the stored secret");
    let kept = change.seal(&ring(), row()).expect("sealed");
    assert_eq!(
        kept.client_secret(&ring(), row())
            .expect("opens")
            .expose_secret(),
        SECRET
    );
}

#[test]
fn the_stored_config_holds_the_secret_only_sealed_and_reads_back() {
    let config = validate(&minimal(), POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed");
    let stored = config.to_json();
    let text = serde_json::to_string(&stored).expect("JSON");

    assert!(!text.contains(SECRET), "{text}");
    assert!(stored["client_secret"]["sealed"].is_string(), "{text}");
    let read = OidcConfig::from_stored(&stored).expect("reads back");
    assert_eq!(read, config);
    assert_eq!(
        read.client_secret(&ring(), row())
            .expect("opens")
            .expose_secret(),
        SECRET
    );
}

#[test]
fn a_sealed_secret_opens_only_for_its_own_row() {
    let config = validate(&minimal(), POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed");
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
            config.client_secret(&ring(), wrong).err(),
            Some(DecryptError::Inauthentic),
            "{wrong:?}"
        );
    }
}

#[test]
fn nothing_prints_the_secret() {
    let draft = validate(&minimal(), POLICY, None).expect("valid");
    let config = draft.clone().seal(&ring(), row()).expect("sealed");
    let opened = config.client_secret(&ring(), row()).expect("opens");
    let shown = format!("{draft:?} {config:?} {opened:?}");
    assert!(!shown.contains(SECRET), "{shown}");
    assert!(!shown.contains("sealed\""), "{shown}");
}

#[test]
fn a_stored_config_with_extra_members_reads_and_a_broken_one_fails() {
    let config = validate(&minimal(), POLICY, None)
        .expect("valid")
        .seal(&ring(), row())
        .expect("sealed");
    let mut stored = config.to_json();
    stored.insert("label".into(), json!("Sign in with Example"));
    assert_eq!(OidcConfig::from_stored(&stored), Ok(config.clone()));

    let mut plain_secret = config.to_json();
    plain_secret.insert("client_secret".into(), json!(SECRET));
    assert_eq!(
        OidcConfig::from_stored(&plain_secret),
        Err(StoredConfigError::Invalid {
            pointer: "/client_secret".into(),
            code: code::FIELD_TYPE,
        })
    );
    let mut no_issuer = config.to_json();
    no_issuer.remove("issuer");
    assert_eq!(
        OidcConfig::from_stored(&no_issuer),
        Err(StoredConfigError::Invalid {
            pointer: "/issuer".into(),
            code: code::FIELD_REQUIRED,
        })
    );
}

#[test]
fn a_stored_config_is_read_without_the_url_policy() {
    // A provider saved under the insecure switch still reads; the client refuses its URL at
    // connect time once the switch is off.
    let insecure = UrlPolicy::new(true, false);
    let config = validate(
        &with("issuer", json!("http://127.0.0.1:9000/")),
        insecure,
        None,
    )
    .expect("valid")
    .seal(&ring(), row())
    .expect("sealed");
    assert!(OidcConfig::from_stored(&config.to_json()).is_ok());
}

#[test]
fn the_oidc_mapping_reads_claims_through_the_shared_claim_mapping() {
    let config = with(
        "claims",
        json!({ "groups": "/groups", "attributes": { "department": "/department" } }),
    );
    let mapping = validate(&config, POLICY, None)
        .expect("valid")
        .settings
        .claims
        .profile();
    let claims = json!({
        "sub": "user-1",
        "given_name": "Test",
        "family_name": 7,
        "email": "student@example.test",
        "email_verified": true,
        "groups": ["staff"],
        "department": "training",
    });
    let mapped = crate::claims::profile(&claims, &mapping);
    assert_eq!(mapped.given_name.as_deref(), Some("Test"));
    assert_eq!(mapped.family_name, None);
    assert_eq!(mapped.email.as_deref(), Some("student@example.test"));
    assert_eq!(mapped.email_verified, Some(true));
    assert_eq!(mapped.groups, Some(vec!["staff".to_owned()]));
    assert_eq!(
        Value::Object(mapped.attributes),
        json!({ "department": "training" })
    );
}

/// The context is written out here, not taken from the code under test, so a change to it
/// (which would leave every stored secret unreadable) turns this red.
#[test]
fn the_secret_is_sealed_to_the_literal_row_context() {
    use base64::Engine as _;
    let draft: OidcConfigDraft = validate(&minimal(), POLICY, None).expect("valid");
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
