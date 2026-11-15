// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers shared by the server's integration tests. A test binary takes them with `mod common;`.
//!
//! [`schema_errors`] checks an answer against a schema of the OpenAPI document (spec 03 T17 and
//! the specs' later "validated against the OpenAPI schema" cases). It reads the subset of JSON
//! Schema utoipa writes: `$ref`, `type` (one name or a list), `enum`, `format` (`uuid`,
//! `date-time` as the API writes it, `int32`, `int64`, `double`), `minimum`, `properties` with `required`,
//! `additionalProperties` (a schema every undeclared member conforms to), `propertyNames`,
//! `items` and `oneOf` (exactly one of the schemas conforms; utoipa writes no other constraint
//! beside it), plus the annotations `description`, `title` and `example`, which constrain nothing.
//! Any other keyword, another `format`, or a `minimum` that is not a number is reported as an
//! error rather than skipped, so a schema the checker cannot read never passes by default. A
//! member the schema does not declare is an error too, so an answer cannot carry more than the
//! document says.

use serde_json::{Map, Value};
use stepclimb_core::id::Uuid;

/// The keywords [`conforms`] checks.
const CHECKED: &[&str] = &[
    "$ref",
    "type",
    "enum",
    "format",
    "minimum",
    "properties",
    "required",
    "additionalProperties",
    "propertyNames",
    "items",
    "oneOf",
];

/// Keywords that annotate a schema and constrain nothing.
const ANNOTATIONS: &[&str] = &["description", "title", "example"];

/// Every way `value` breaks the component schema `name` of `document`, each with the JSON
/// Pointer of the member at fault. Empty when it conforms.
pub fn schema_errors(document: &Value, name: &str, value: &Value) -> Vec<String> {
    let schema = resolve(document, &format!("#/components/schemas/{name}"));
    let mut errors = Vec::new();
    conforms(document, schema, value, "", &mut errors);
    errors
}

fn resolve<'a>(document: &'a Value, reference: &str) -> &'a Value {
    let pointer = reference.strip_prefix('#').expect("a local reference");
    document
        .pointer(pointer)
        .unwrap_or_else(|| panic!("{reference} is not in the document"))
}

fn conforms(document: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        errors.push(format!("{at}: the schema is not an object"));
        return;
    };
    if let Some(unknown) = schema
        .keys()
        .find(|k| !CHECKED.contains(&k.as_str()) && !ANNOTATIONS.contains(&k.as_str()))
    {
        errors.push(format!(
            "{at}: the checker does not read the keyword {unknown:?}"
        ));
        return;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return conforms(document, resolve(document, reference), value, at, errors);
    }
    if let Some(choices) = schema.get("oneOf") {
        return one_of_conforms(document, choices, value, at, errors);
    }
    if !type_conforms(schema, value, at, errors) {
        return;
    }
    enum_conforms(schema, value, at, errors);
    format_conforms(schema, value, at, errors);
    minimum_conforms(schema, value, at, errors);
    object_conforms(document, schema, value, at, errors);
    if let (Some(items), Some(item_schema)) = (value.as_array(), schema.get("items")) {
        for (index, item) in items.iter().enumerate() {
            conforms(
                document,
                item_schema,
                item,
                &format!("{at}/{index}"),
                errors,
            );
        }
    }
}

/// `value` conforms to exactly one of `choices`, an array of schemas.
fn one_of_conforms(
    document: &Value,
    choices: &Value,
    value: &Value,
    at: &str,
    errors: &mut Vec<String>,
) {
    let Some(choices) = choices.as_array() else {
        errors.push(format!("{at}: oneOf is not an array"));
        return;
    };
    let fitting = choices
        .iter()
        .filter(|choice| {
            let mut found = Vec::new();
            conforms(document, choice, value, at, &mut found);
            found.is_empty()
        })
        .count();
    if fitting != 1 {
        errors.push(format!(
            "{at}: {value} conforms to {fitting} of the oneOf schemas, not exactly one"
        ));
    }
}

/// `false` when `value` is none of the schema's types, after which nothing else is checked.
fn type_conforms(
    schema: &Map<String, Value>,
    value: &Value,
    at: &str,
    errors: &mut Vec<String>,
) -> bool {
    let Some(types) = schema.get("type") else {
        return true;
    };
    let names: Vec<&str> = match types {
        Value::String(name) => vec![name.as_str()],
        Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    let matches = |name: &str| match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    if names.into_iter().any(matches) {
        return true;
    }
    errors.push(format!("{at}: {value} is not of type {types}"));
    false
}

fn enum_conforms(schema: &Map<String, Value>, value: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        errors.push(format!("{at}: {value} is not one of {allowed:?}"));
    }
}

fn format_conforms(schema: &Map<String, Value>, value: &Value, at: &str, errors: &mut Vec<String>) {
    let Some(format) = schema.get("format").and_then(Value::as_str) else {
        return;
    };
    let fits = match format {
        "uuid" => value
            .as_str()
            .is_none_or(|text| Uuid::try_parse(text).is_ok()),
        "date-time" => value.as_str().is_none_or(is_api_timestamp),
        "int32" => !value.is_number() || value.as_i64().is_some_and(|n| i32::try_from(n).is_ok()),
        "int64" => !value.is_number() || value.is_i64(),
        // Any JSON number is a double.
        "double" => true,
        other => {
            errors.push(format!("{at}: the checker does not read format {other:?}"));
            return;
        }
    };
    if !fits {
        errors.push(format!("{at}: {value} is not a {format}"));
    }
}

fn minimum_conforms(
    schema: &Map<String, Value>,
    value: &Value,
    at: &str,
    errors: &mut Vec<String>,
) {
    let Some(minimum) = schema.get("minimum") else {
        return;
    };
    let Some(minimum) = minimum.as_f64() else {
        errors.push(format!(
            "{at}: the schema's minimum {minimum} is not a number"
        ));
        return;
    };
    let Some(number) = value.as_f64() else {
        return;
    };
    if number < minimum {
        errors.push(format!("{at}: {value} is below the minimum {minimum}"));
    }
}

/// RFC 3339 as `stepclimb_server::timestamp` writes it: `2025-11-13T02:13:20.123456Z`.
fn is_api_timestamp(text: &str) -> bool {
    const SHAPE: &[u8] = b"dddd-dd-ddTdd:dd:dd.ddddddZ";
    text.len() == SHAPE.len()
        && text
            .bytes()
            .zip(SHAPE)
            .all(|(byte, expected)| match expected {
                b'd' => byte.is_ascii_digit(),
                other => byte == *other,
            })
}

fn object_conforms(
    document: &Value,
    schema: &Map<String, Value>,
    value: &Value,
    at: &str,
    errors: &mut Vec<String>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    let empty = Map::new();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let required = schema.get("required").and_then(Value::as_array);
    for name in required.into_iter().flatten().filter_map(Value::as_str) {
        if !object.contains_key(name) {
            errors.push(format!("{at}/{name}: required but missing"));
        }
    }
    let additional = schema.get("additionalProperties").filter(|s| s.is_object());
    for (name, member) in object {
        let member_at = format!("{at}/{name}");
        if let Some(names) = schema.get("propertyNames") {
            conforms(
                document,
                names,
                &Value::String(name.clone()),
                &member_at,
                errors,
            );
        }
        match properties.get(name).or(additional) {
            Some(property) => conforms(document, property, member, &member_at, errors),
            None => errors.push(format!("{member_at}: not declared in the schema")),
        }
    }
}
