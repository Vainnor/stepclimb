// SPDX-License-Identifier: AGPL-3.0-or-later
//! The schema checker of `tests/common` must be able to fail: each test breaks one thing a
//! conforming `Me` or `Entitlements` answer or the document's schema has and expects an error
//! for it. No database.

use serde_json::{Value, json};
use stepclimb_server::openapi;

mod common;

fn document() -> Value {
    serde_json::to_value(openapi::document()).expect("the document as JSON")
}

/// A `Me` answer with every member the schema allows, a grant and a feature included.
fn good() -> Value {
    json!({
        "user": {
            "id": "0190a000-0000-7000-8000-000000000000", "display_name": "Sam Ortiz",
            "given_name": "Sam", "family_name": "Ortiz", "email": null, "locale": "en",
            "tz": "UTC", "status": "active", "source": "local",
        },
        "organization": {
            "id": "0190a000-0000-7000-8000-000000000001", "slug": "example", "name": "Example Org",
        },
        "grants": [{
            "scope": { "type": "account", "id": "0190a000-0000-7000-8000-000000000002" },
            "permissions": ["account.view"],
        }],
        "features": ["custom_roles"],
    })
}

fn errors_for(value: &Value) -> Vec<String> {
    common::schema_errors(&document(), "Me", value)
}

#[test]
fn a_conforming_answer_has_no_errors() {
    let errors = errors_for(&good());
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn a_wrong_type_enum_value_or_uuid_is_an_error() {
    let broken = [
        ("/user/tz", Value::Null),
        ("/user/status", json!("deleted")),
        ("/grants/0/scope/type", json!("planet")),
        ("/organization/id", json!("not-a-uuid")),
        ("/features", json!("custom_roles")),
        ("/grants/0/permissions/0", json!(7)),
    ];
    for (pointer, replacement) in broken {
        let mut value = good();
        *value.pointer_mut(pointer).expect("a member") = replacement;
        assert!(!errors_for(&value).is_empty(), "{pointer} was accepted");
    }
}

#[test]
fn a_missing_or_undeclared_member_is_an_error() {
    let mut extra = good();
    extra["user"]["external_id"] = json!("E1");
    assert!(
        !errors_for(&extra).is_empty(),
        "an undeclared member was accepted"
    );
    let mut missing = good();
    missing["user"]
        .as_object_mut()
        .expect("an object")
        .remove("email");
    assert!(
        !errors_for(&missing).is_empty(),
        "a missing member was accepted"
    );
}

#[test]
fn a_keyword_or_format_the_checker_does_not_read_is_an_error() {
    let cases = [
        ("pattern", json!("^[A-Z]+$")),
        ("minLength", json!(100)),
        ("format", json!("email")),
    ];
    for (keyword, constraint) in cases {
        let mut document = document();
        document["components"]["schemas"]["MeUser"]["properties"]["tz"][keyword] = constraint;
        let errors = common::schema_errors(&document, "Me", &good());
        assert!(!errors.is_empty(), "{keyword} was skipped");
    }
}

#[test]
fn annotations_constrain_nothing() {
    let mut document = document();
    document["components"]["schemas"]["MeUser"]["properties"]["tz"]["example"] = json!("UTC");
    document["components"]["schemas"]["MeUser"]["title"] = json!("The user");
    let errors = common::schema_errors(&document, "Me", &good());
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn a_timestamp_or_integer_outside_its_format_is_an_error() {
    let mut document = document();
    let user = &mut document["components"]["schemas"]["MeUser"]["properties"];
    user["tz"]["format"] = json!("date-time");
    user["locale"] = json!({ "type": "integer", "format": "int32" });
    let errors_with = |tz: Value, locale: Value| {
        let mut value = good();
        value["user"]["tz"] = tz;
        value["user"]["locale"] = locale;
        common::schema_errors(&document, "Me", &value)
    };
    let fine = errors_with(json!("2025-11-13T02:13:20.123456Z"), json!(16));
    assert!(fine.is_empty(), "{fine:#?}");
    for tz in [
        "2025-11-13T02:13:20Z",
        "2025-11-13 02:13:20.123456Z",
        "2025-11-13T02:13:20.123456+00:00",
        "UTC",
    ] {
        assert!(
            !errors_with(json!(tz), json!(16)).is_empty(),
            "{tz} was accepted"
        );
    }
    for locale in [
        json!(2_147_483_648_i64),
        json!(-2_147_483_649_i64),
        json!(1.5),
    ] {
        assert!(
            !errors_with(json!("2025-11-13T02:13:20.123456Z"), locale.clone()).is_empty(),
            "{locale} was accepted"
        );
    }
}

/// An `Entitlements` answer: a map of features and a map of limits, one with a value.
fn good_entitlements() -> Value {
    json!({
        "plan": { "key": "unlimited", "source": "selfhost" },
        "features": { "custom_roles": true, "api_keys": false },
        "limits": { "users": 5, "storage": null },
    })
}

#[test]
fn a_map_member_is_checked_against_its_value_schema() {
    let check = |value: &Value| common::schema_errors(&document(), "Entitlements", value);
    let errors = check(&good_entitlements());
    assert!(errors.is_empty(), "{errors:#?}");
    let broken = [
        ("/features/custom_roles", json!("yes")),
        ("/limits/users", json!(-1)),
        ("/limits/users", json!("5")),
        ("/plan/source", json!("trial")),
    ];
    for (pointer, replacement) in broken {
        let mut value = good_entitlements();
        *value.pointer_mut(pointer).expect("a member") = replacement.clone();
        assert!(
            !check(&value).is_empty(),
            "{pointer} = {replacement} was accepted"
        );
    }
    let mut extra = good_entitlements();
    extra["plan"]["ends_at"] = json!(null);
    assert!(
        !check(&extra).is_empty(),
        "an undeclared member was accepted"
    );
}

#[test]
fn property_names_are_checked() {
    let mut document = document();
    document["components"]["schemas"]["Entitlements"]["properties"]["features"]["propertyNames"] =
        json!({ "type": "string", "enum": ["custom_roles"] });
    let errors = common::schema_errors(&document, "Entitlements", &good_entitlements());
    assert!(
        errors.iter().any(|e| e.starts_with("/features/api_keys")),
        "{errors:#?}"
    );
}
