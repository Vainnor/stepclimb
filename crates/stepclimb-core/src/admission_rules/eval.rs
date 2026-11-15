// SPDX-License-Identifier: AGPL-3.0-or-later
//! Evaluating a rule against a sign-in's mapped attributes (docs/specs/m0/07-admission.md B9).
//!
//! Every path that is not a clear "holds" refuses:
//!
//! - an attribute that is absent, or present as JSON `null`, makes its condition fail, whatever
//!   the test: `exists` fails, and so does `not_in`, since an absent value is not known to be
//!   outside the list;
//! - a present attribute of a type the test cannot compare (a number where the rule has a
//!   string, an array, an object, a non-integer number against an integer) makes the rule
//!   [`Outcome::Unevaluable`];
//! - a group evaluates every rule in it, so an unevaluable rule anywhere inside makes the whole
//!   group unevaluable, even an `any` where another rule holds. The result never depends on the
//!   order of the rules.

use serde_json::{Map, Value};

use super::model::{Condition, Rule, Scalar, Test};

/// What a rule said about a sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Holds,
    Fails,
    /// An attribute has a type the rule cannot compare; the sign-in is refused.
    Unevaluable,
}

/// A value the rule cannot compare.
struct Unevaluable;

/// What `rule` says about a sign-in with `attributes`.
#[must_use]
pub fn evaluate(rule: &Rule, attributes: &Map<String, Value>) -> Outcome {
    match holds(rule, attributes) {
        Ok(true) => Outcome::Holds,
        Ok(false) => Outcome::Fails,
        Err(Unevaluable) => Outcome::Unevaluable,
    }
}

fn holds(rule: &Rule, attributes: &Map<String, Value>) -> Result<bool, Unevaluable> {
    match rule {
        Rule::Condition(condition) => condition_holds(condition, attributes),
        Rule::Any(rules) => Ok(each(rules, attributes)?.contains(&true)),
        Rule::All(rules) => Ok(!each(rules, attributes)?.contains(&false)),
    }
}

/// Every rule of a group, evaluated: unevaluable when any one is.
fn each(rules: &[Rule], attributes: &Map<String, Value>) -> Result<Vec<bool>, Unevaluable> {
    let results: Vec<Result<bool, Unevaluable>> =
        rules.iter().map(|rule| holds(rule, attributes)).collect();
    results.into_iter().collect()
}

fn condition_holds(
    condition: &Condition,
    attributes: &Map<String, Value>,
) -> Result<bool, Unevaluable> {
    let Some(value) = attributes
        .get(&condition.attribute)
        .filter(|value| !value.is_null())
    else {
        return Ok(false);
    };
    match &condition.test {
        Test::Exists => Ok(true),
        Test::Equals(scalar) => equals(scalar, value),
        Test::In(values) => one_of(values, value),
        Test::NotIn(values) => one_of(values, value).map(|found| !found),
        Test::Matches(pattern) => value
            .as_str()
            .map(|text| pattern.matches(text))
            .ok_or(Unevaluable),
    }
}

fn one_of(values: &[Scalar], value: &Value) -> Result<bool, Unevaluable> {
    let found: Vec<bool> = values
        .iter()
        .map(|scalar| equals(scalar, value))
        .collect::<Result<_, _>>()?;
    Ok(found.contains(&true))
}

/// Whether `value` is `scalar`; unevaluable when it is not of `scalar`'s type. An integer the
/// rule cannot hold (above `i64::MAX`) is a different integer, so not equal.
fn equals(scalar: &Scalar, value: &Value) -> Result<bool, Unevaluable> {
    match (scalar, value) {
        (Scalar::Text(want), Value::String(have)) => Ok(want == have),
        (Scalar::Boolean(want), Value::Bool(have)) => Ok(want == have),
        (Scalar::Integer(want), Value::Number(have)) => match have.as_i64() {
            Some(have) => Ok(*want == have),
            None if have.is_u64() => Ok(false),
            None => Err(Unevaluable),
        },
        _ => Err(Unevaluable),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::model::parse_list;
    use super::*;

    fn rule(value: Value) -> Rule {
        parse_list(&[value], &[], None)
            .expect("a valid rule")
            .remove(0)
    }

    fn attrs(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("an object"),
        }
    }

    fn outcome(r: &Value, a: Value) -> Outcome {
        evaluate(&rule(r.clone()), &attrs(a))
    }

    #[test]
    fn equals_compares_values_of_the_same_type() {
        let usa = json!({ "attribute": "subdivision", "op": "equals", "value": "USA" });
        assert_eq!(
            outcome(&usa, json!({ "subdivision": "USA" })),
            Outcome::Holds
        );
        assert_eq!(
            outcome(&usa, json!({ "subdivision": "usa" })),
            Outcome::Fails
        );
        assert_eq!(
            outcome(&usa, json!({ "subdivision": 1 })),
            Outcome::Unevaluable
        );
        assert_eq!(
            outcome(&usa, json!({ "subdivision": ["USA"] })),
            Outcome::Unevaluable
        );
        let five = json!({ "attribute": "rating", "op": "equals", "value": 5 });
        assert_eq!(outcome(&five, json!({ "rating": 5 })), Outcome::Holds);
        assert_eq!(outcome(&five, json!({ "rating": 4 })), Outcome::Fails);
        assert_eq!(
            outcome(&five, json!({ "rating": "5" })),
            Outcome::Unevaluable
        );
        assert_eq!(
            outcome(&five, json!({ "rating": 5.0 })),
            Outcome::Unevaluable
        );
        assert_eq!(
            outcome(&five, json!({ "rating": u64::MAX })),
            Outcome::Fails
        );
        let staff = json!({ "attribute": "staff", "op": "equals", "value": true });
        assert_eq!(outcome(&staff, json!({ "staff": true })), Outcome::Holds);
        assert_eq!(outcome(&staff, json!({ "staff": false })), Outcome::Fails);
        assert_eq!(
            outcome(&staff, json!({ "staff": "true" })),
            Outcome::Unevaluable
        );
    }

    #[test]
    fn an_absent_or_null_attribute_fails_every_test() {
        for r in [
            json!({ "attribute": "a", "op": "equals", "value": "x" }),
            json!({ "attribute": "a", "op": "in", "value": ["x"] }),
            json!({ "attribute": "a", "op": "not_in", "value": ["x"] }),
            json!({ "attribute": "a", "op": "exists" }),
            json!({ "attribute": "a", "op": "matches", "value": ".*" }),
        ] {
            assert_eq!(outcome(&r, json!({})), Outcome::Fails, "{r}");
            assert_eq!(outcome(&r, json!({ "a": null })), Outcome::Fails, "{r}");
            assert_eq!(outcome(&r, json!({ "b": "x" })), Outcome::Fails, "{r}");
        }
    }

    #[test]
    fn in_and_not_in_look_the_value_up() {
        let within = json!({ "attribute": "rating", "op": "in", "value": ["S1", "S2"] });
        let outside = json!({ "attribute": "rating", "op": "not_in", "value": ["S1", "S2"] });
        assert_eq!(outcome(&within, json!({ "rating": "S2" })), Outcome::Holds);
        assert_eq!(outcome(&within, json!({ "rating": "S3" })), Outcome::Fails);
        assert_eq!(outcome(&outside, json!({ "rating": "S3" })), Outcome::Holds);
        assert_eq!(outcome(&outside, json!({ "rating": "S1" })), Outcome::Fails);
        assert_eq!(
            outcome(&within, json!({ "rating": 2 })),
            Outcome::Unevaluable
        );
        assert_eq!(
            outcome(&outside, json!({ "rating": 2 })),
            Outcome::Unevaluable
        );
    }

    #[test]
    fn exists_holds_for_any_present_value() {
        let r = json!({ "attribute": "a", "op": "exists" });
        for value in [json!(""), json!(0), json!(false), json!([]), json!({})] {
            assert_eq!(
                outcome(&r, json!({ "a": value })),
                Outcome::Holds,
                "{value}"
            );
        }
    }

    #[test]
    fn matches_needs_a_string_matched_whole() {
        let r = json!({ "attribute": "cid", "op": "matches", "value": "[0-9]{7}" });
        assert_eq!(outcome(&r, json!({ "cid": "1000001" })), Outcome::Holds);
        assert_eq!(outcome(&r, json!({ "cid": "10000011" })), Outcome::Fails);
        assert_eq!(
            outcome(&r, json!({ "cid": 1_000_001 })),
            Outcome::Unevaluable
        );
    }

    #[test]
    fn groups_combine_their_rules() {
        let any = json!({ "any": [
            { "attribute": "a", "op": "equals", "value": "x" },
            { "attribute": "b", "op": "equals", "value": "y" }
        ] });
        let all = json!({ "all": [
            { "attribute": "a", "op": "equals", "value": "x" },
            { "attribute": "b", "op": "equals", "value": "y" }
        ] });
        assert_eq!(outcome(&any, json!({ "a": "x" })), Outcome::Holds);
        assert_eq!(outcome(&any, json!({ "b": "y" })), Outcome::Holds);
        assert_eq!(outcome(&any, json!({ "a": "z", "b": "z" })), Outcome::Fails);
        assert_eq!(outcome(&all, json!({ "a": "x", "b": "y" })), Outcome::Holds);
        assert_eq!(outcome(&all, json!({ "a": "x" })), Outcome::Fails);
        assert_eq!(outcome(&all, json!({ "a": "x", "b": "z" })), Outcome::Fails);
    }

    #[test]
    fn an_unevaluable_rule_anywhere_in_a_group_makes_it_unevaluable() {
        let any = json!({ "any": [
            { "attribute": "a", "op": "equals", "value": "x" },
            { "attribute": "b", "op": "equals", "value": "y" }
        ] });
        assert_eq!(
            outcome(&any, json!({ "a": "x", "b": 1 })),
            Outcome::Unevaluable
        );
        assert_eq!(
            outcome(&any, json!({ "a": 1, "b": "y" })),
            Outcome::Unevaluable
        );
        let all = json!({ "all": [
            { "attribute": "a", "op": "equals", "value": "x" },
            { "any": [{ "attribute": "b", "op": "matches", "value": "y" }] }
        ] });
        assert_eq!(
            outcome(&all, json!({ "a": "z", "b": [] })),
            Outcome::Unevaluable
        );
    }
}
