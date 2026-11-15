// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rule model and its JSON form (docs/specs/m0/07-admission.md B8, Q1 (b)).
//!
//! A rule list is a JSON array; every rule in it must hold. A rule is one of:
//!
//! | Form | Holds when |
//! | --- | --- |
//! | `{"attribute": a, "op": "equals", "value": v}` | attribute `a` is `v` |
//! | `{"attribute": a, "op": "in", "value": [v, ...]}` | attribute `a` is one of the values |
//! | `{"attribute": a, "op": "not_in", "value": [v, ...]}` | attribute `a` is present and none of the values |
//! | `{"attribute": a, "op": "exists"}` | attribute `a` is present |
//! | `{"attribute": a, "op": "matches", "value": p}` | attribute `a` is a string the regular expression `p` matches whole |
//! | `{"any": [rule, ...]}` | at least one of the rules holds |
//! | `{"all": [rule, ...]}` | every one of the rules holds |
//!
//! A value `v` is a string, an integer or a boolean; the values of one `in` or `not_in` list
//! are all of one of those types. A pattern is anchored at both ends: `S[1-3]` matches `S2`
//! and not `S20`. [`parse_list`] refuses anything else, every member it does not know
//! included, and lists every problem it finds as a [`Problem`]: a path to the member and a
//! code. [`to_json`] writes the form back; parsing what it wrote gives the same rules.

use std::collections::BTreeSet;
use std::fmt;

use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value, json};

/// The codes of [`Problem`]s.
pub mod code {
    /// A rule that is not one of the forms, or breaks a limit.
    pub const RULE_INVALID: &str = "admission.rule_invalid";
    /// A rule naming an attribute the provider does not map.
    pub const ATTRIBUTE_UNKNOWN: &str = "admission.attribute_unknown";
    /// A provider list for a key that is not an outside provider of the organization.
    pub const PROVIDER_UNKNOWN: &str = "admission.provider_unknown";
}

/// The most rules in one list.
pub const MAX_RULES: usize = 50;
/// The most rules in one `any` or `all` group.
pub const MAX_GROUP_RULES: usize = 50;
/// The most groups nested inside each other.
pub const MAX_DEPTH: usize = 4;
/// The most values in one `in` or `not_in` list.
pub const MAX_VALUES: usize = 100;
/// The most characters in an attribute name, a string value or a pattern.
pub const MAX_TEXT_CHARS: usize = 1000;
/// The most memory, in bytes, one compiled pattern may take.
const MAX_PATTERN_BYTES: usize = 1 << 20;

/// One admission rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    Condition(Condition),
    /// Holds when at least one of the rules holds.
    Any(Vec<Rule>),
    /// Holds when every one of the rules holds.
    All(Vec<Rule>),
}

/// A test of one attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    /// The name of a mapped attribute (`05-oidc.md` B13, `06-oauth2-and-vatsim-preset.md` B6).
    pub attribute: String,
    pub test: Test,
}

/// What a [`Condition`] asks of its attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Test {
    Equals(Scalar),
    In(Vec<Scalar>),
    NotIn(Vec<Scalar>),
    Exists,
    Matches(Pattern),
}

impl Test {
    /// The `op` the JSON form names.
    #[must_use]
    pub fn op(&self) -> &'static str {
        match self {
            Self::Equals(_) => "equals",
            Self::In(_) => "in",
            Self::NotIn(_) => "not_in",
            Self::Exists => "exists",
            Self::Matches(_) => "matches",
        }
    }
}

/// A value a rule compares an attribute with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scalar {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

impl Scalar {
    fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "string",
            Self::Integer(_) => "integer",
            Self::Boolean(_) => "boolean",
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Self::Text(text) => Value::from(text.as_str()),
            Self::Integer(n) => Value::from(*n),
            Self::Boolean(b) => Value::from(*b),
        }
    }
}

/// A `matches` pattern: the source the admin wrote and the regular expression compiled from
/// it, anchored at both ends. Two patterns are equal when their sources are.
#[derive(Clone)]
pub struct Pattern {
    source: String,
    regex: Regex,
}

impl Pattern {
    /// `source` compiled with `^(?:` and `)$` around it, or `None` when it is no regular
    /// expression the `regex` crate accepts within [`MAX_PATTERN_BYTES`].
    fn compile(source: &str) -> Option<Self> {
        let regex = RegexBuilder::new(&format!("^(?:{source})$"))
            .size_limit(MAX_PATTERN_BYTES)
            .build()
            .ok()?;
        Some(Self {
            source: source.to_owned(),
            regex,
        })
    }

    /// The pattern as the admin wrote it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether the pattern matches the whole of `text`.
    #[must_use]
    pub fn matches(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Pattern {}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Pattern").field(&self.source).finish()
    }
}

/// One rule a list broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The path to the member, one segment per object key or array index, from the root the
    /// caller named: `["rules", "organization", "0", "op"]`.
    pub path: Vec<String>,
    /// One of [`code`].
    pub code: &'static str,
}

/// The rules of the JSON array `rules`, or every problem in it. `path` is where the list sits,
/// the start of every [`Problem::path`]. With `known`, a condition naming an attribute not in
/// it is [`code::ATTRIBUTE_UNKNOWN`]; without, any attribute name is accepted, as for a stored
/// list whose provider has since stopped mapping one (evaluation then finds it absent).
pub fn parse_list(
    rules: &[Value],
    path: &[String],
    known: Option<&BTreeSet<String>>,
) -> Result<Vec<Rule>, Vec<Problem>> {
    let mut walk = Walk {
        problems: Vec::new(),
        known,
    };
    if rules.len() > MAX_RULES {
        walk.problem(path, code::RULE_INVALID);
    }
    let parsed: Vec<Option<Rule>> = rules
        .iter()
        .enumerate()
        .map(|(i, rule)| walk.rule(rule, &child(path, i), 0))
        .collect();
    if walk.problems.is_empty() {
        Ok(parsed.into_iter().flatten().collect())
    } else {
        Err(walk.problems)
    }
}

/// The JSON array [`parse_list`] reads back as `rules`.
#[must_use]
pub fn to_json(rules: &[Rule]) -> Value {
    Value::Array(rules.iter().map(rule_json).collect())
}

fn rule_json(rule: &Rule) -> Value {
    match rule {
        Rule::Any(rules) => json!({ "any": to_json(rules) }),
        Rule::All(rules) => json!({ "all": to_json(rules) }),
        Rule::Condition(condition) => {
            let mut map = Map::new();
            map.insert("attribute".into(), condition.attribute.as_str().into());
            map.insert("op".into(), condition.test.op().into());
            let value = match &condition.test {
                Test::Exists => None,
                Test::Equals(scalar) => Some(scalar.to_json()),
                Test::In(values) | Test::NotIn(values) => {
                    Some(Value::Array(values.iter().map(Scalar::to_json).collect()))
                }
                Test::Matches(pattern) => Some(pattern.source.as_str().into()),
            };
            if let Some(value) = value {
                map.insert("value".into(), value);
            }
            Value::Object(map)
        }
    }
}

/// `path` with `segment` after it.
fn child(path: &[String], segment: impl ToString) -> Vec<String> {
    let mut child = path.to_vec();
    child.push(segment.to_string());
    child
}

struct Walk<'a> {
    problems: Vec<Problem>,
    known: Option<&'a BTreeSet<String>>,
}

impl Walk<'_> {
    fn problem(&mut self, path: &[String], code: &'static str) {
        self.problems.push(Problem {
            path: path.to_vec(),
            code,
        });
    }

    /// The rule at `path`, `depth` groups deep, or `None` after recording its problems.
    fn rule(&mut self, value: &Value, path: &[String], depth: usize) -> Option<Rule> {
        let Value::Object(map) = value else {
            self.problem(path, code::RULE_INVALID);
            return None;
        };
        for group in ["any", "all"] {
            if let Some(rules) = map.get(group) {
                self.unknown_members(map, &[group], path);
                let rules = self.group(rules, &child(path, group), depth + 1)?;
                return Some(if group == "any" {
                    Rule::Any(rules)
                } else {
                    Rule::All(rules)
                });
            }
        }
        self.condition(map, path).map(Rule::Condition)
    }

    /// Record every member of `map` not in `allowed`.
    fn unknown_members(&mut self, map: &Map<String, Value>, allowed: &[&str], path: &[String]) {
        for key in map.keys().filter(|key| !allowed.contains(&key.as_str())) {
            self.problem(&child(path, key), code::RULE_INVALID);
        }
    }

    /// The rules of an `any` or `all` group at `path`, the group `depth` deep.
    fn group(&mut self, value: &Value, path: &[String], depth: usize) -> Option<Vec<Rule>> {
        let Value::Array(rules) = value else {
            self.problem(path, code::RULE_INVALID);
            return None;
        };
        if rules.is_empty() || rules.len() > MAX_GROUP_RULES || depth > MAX_DEPTH {
            self.problem(path, code::RULE_INVALID);
            return None;
        }
        let parsed: Vec<Option<Rule>> = rules
            .iter()
            .enumerate()
            .map(|(i, rule)| self.rule(rule, &child(path, i), depth))
            .collect();
        parsed.into_iter().collect()
    }

    fn condition(&mut self, map: &Map<String, Value>, path: &[String]) -> Option<Condition> {
        self.unknown_members(map, &["attribute", "op", "value"], path);
        let attribute = self.attribute(map.get("attribute"), &child(path, "attribute"));
        let value = map.get("value");
        let value_path = child(path, "value");
        let test = match map.get("op").and_then(Value::as_str) {
            Some("exists") if value.is_none() => Some(Test::Exists),
            Some("exists") => {
                self.problem(&value_path, code::RULE_INVALID);
                None
            }
            Some("equals") => self.scalar(value, &value_path).map(Test::Equals),
            Some("in") => self.values(value, &value_path).map(Test::In),
            Some("not_in") => self.values(value, &value_path).map(Test::NotIn),
            Some("matches") => self.pattern(value, &value_path).map(Test::Matches),
            _ => {
                self.problem(&child(path, "op"), code::RULE_INVALID);
                None
            }
        };
        Some(Condition {
            attribute: attribute?,
            test: test?,
        })
    }

    fn attribute(&mut self, value: Option<&Value>, path: &[String]) -> Option<String> {
        let Some(name) = value
            .and_then(Value::as_str)
            .filter(|name| usable_text(name))
        else {
            self.problem(path, code::RULE_INVALID);
            return None;
        };
        if self.known.is_some_and(|known| !known.contains(name)) {
            self.problem(path, code::ATTRIBUTE_UNKNOWN);
            return None;
        }
        Some(name.to_owned())
    }

    fn scalar(&mut self, value: Option<&Value>, path: &[String]) -> Option<Scalar> {
        let scalar = match value {
            Some(Value::String(text)) if usable_text(text) || text.is_empty() => {
                Some(Scalar::Text(text.clone()))
            }
            Some(Value::Number(n)) => n.as_i64().map(Scalar::Integer),
            Some(Value::Bool(b)) => Some(Scalar::Boolean(*b)),
            _ => None,
        };
        if scalar.is_none() {
            self.problem(path, code::RULE_INVALID);
        }
        scalar
    }

    /// The values of an `in` or `not_in` list: one to [`MAX_VALUES`] scalars of one type.
    fn values(&mut self, value: Option<&Value>, path: &[String]) -> Option<Vec<Scalar>> {
        let Some(Value::Array(items)) = value else {
            self.problem(path, code::RULE_INVALID);
            return None;
        };
        if items.is_empty() || items.len() > MAX_VALUES {
            self.problem(path, code::RULE_INVALID);
            return None;
        }
        let scalars: Vec<Option<Scalar>> = items
            .iter()
            .enumerate()
            .map(|(i, item)| self.scalar(Some(item), &child(path, i)))
            .collect();
        let scalars: Vec<Scalar> = scalars.into_iter().collect::<Option<_>>()?;
        let kind = scalars[0].kind();
        if let Some(i) = scalars.iter().position(|s| s.kind() != kind) {
            self.problem(&child(path, i), code::RULE_INVALID);
            return None;
        }
        Some(scalars)
    }

    fn pattern(&mut self, value: Option<&Value>, path: &[String]) -> Option<Pattern> {
        let pattern = value
            .and_then(Value::as_str)
            .filter(|source| usable_text(source))
            .and_then(Pattern::compile);
        if pattern.is_none() {
            self.problem(path, code::RULE_INVALID);
        }
        pattern
    }
}

/// Whether `text` can be a name, a string value or a pattern: one to [`MAX_TEXT_CHARS`]
/// characters, no NUL (which `jsonb` cannot store).
fn usable_text(text: &str) -> bool {
    !text.is_empty() && !text.contains('\0') && text.chars().count() <= MAX_TEXT_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Vec<String> {
        vec!["rules".into()]
    }

    fn parse(value: Value) -> Result<Vec<Rule>, Vec<Problem>> {
        let Value::Array(rules) = value else {
            panic!("a list")
        };
        parse_list(&rules, &root(), None)
    }

    fn problems(value: Value) -> Vec<(String, &'static str)> {
        parse(value)
            .expect_err("refused")
            .into_iter()
            .map(|p| (p.path.join("/"), p.code))
            .collect()
    }

    #[test]
    fn every_form_parses_and_writes_back_the_same() {
        let list = json!([
            { "attribute": "subdivision", "op": "equals", "value": "USA" },
            { "attribute": "rating", "op": "in", "value": [3, 4, 5] },
            { "attribute": "facility", "op": "not_in", "value": ["ZXX"] },
            { "attribute": "member", "op": "exists" },
            { "attribute": "cid", "op": "matches", "value": "[0-9]+" },
            { "any": [
                { "attribute": "staff", "op": "equals", "value": true },
                { "all": [
                    { "attribute": "rating", "op": "equals", "value": 5 },
                    { "attribute": "region", "op": "in", "value": ["AMAS"] }
                ] }
            ] }
        ]);
        let rules = parse(list.clone()).expect("valid");
        assert_eq!(rules.len(), 6);
        assert_eq!(to_json(&rules), list);
        assert_eq!(parse(to_json(&rules)).expect("valid again"), rules);
        assert_eq!(parse(json!([])).expect("an empty list"), Vec::new());
    }

    #[test]
    fn a_pattern_matches_the_whole_value() {
        let pattern = Pattern::compile("S[1-3]").expect("compiles");
        assert!(pattern.matches("S2"));
        assert!(!pattern.matches("S20"));
        assert!(!pattern.matches("xS2"));
        let either = Pattern::compile("a|b").expect("compiles");
        assert!(either.matches("a") && either.matches("b"));
        assert!(
            !either.matches("ab"),
            "the alternation is anchored as a whole"
        );
        assert!(Pattern::compile("(").is_none());
        assert!(
            Pattern::compile("a{1000}{1000}").is_none(),
            "over the size limit"
        );
    }

    #[test]
    fn a_broken_rule_names_the_member() {
        assert_eq!(
            problems(json!(["equals"])),
            [("rules/0".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "like", "value": "x" }])),
            [("rules/0/op".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "op": "exists" }])),
            [("rules/0/attribute".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "exists", "value": 1 }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "equals" }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "matches", "value": "(" }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
    }

    #[test]
    fn an_unknown_member_is_refused() {
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "exists", "negate": true }])),
            [("rules/0/negate".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "any": [{ "attribute": "a", "op": "exists" }], "attribute": "b" }])),
            [("rules/0/attribute".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "any": [{ "attribute": "a", "op": "exists" }], "all": [] }])),
            [("rules/0/all".into(), code::RULE_INVALID)]
        );
    }

    #[test]
    fn values_are_strings_integers_or_booleans_of_one_type() {
        for bad in [
            json!(1.5),
            json!(null),
            json!([1]),
            json!({}),
            json!(u64::MAX),
        ] {
            assert_eq!(
                problems(json!([{ "attribute": "a", "op": "equals", "value": bad }])),
                [("rules/0/value".into(), code::RULE_INVALID)],
                "{bad}"
            );
        }
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "in", "value": ["S1", 2] }])),
            [("rules/0/value/1".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "in", "value": [] }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "not_in", "value": "S1" }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
        assert!(parse(json!([{ "attribute": "a", "op": "equals", "value": "" }])).is_ok());
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "equals", "value": "x\u{0}" }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
    }

    #[test]
    fn limits_are_enforced() {
        let exists = json!({ "attribute": "a", "op": "exists" });
        let many = Value::Array(vec![exists.clone(); MAX_RULES + 1]);
        assert_eq!(problems(many), [("rules".into(), code::RULE_INVALID)]);
        let full = Value::Array(vec![exists.clone(); MAX_RULES]);
        assert!(parse(full).is_ok());

        let mut nested = exists.clone();
        for _ in 0..MAX_DEPTH {
            nested = json!({ "all": [nested] });
        }
        assert!(parse(json!([nested.clone()])).is_ok());
        let too_deep = json!([{ "any": [nested] }]);
        assert_eq!(
            problems(too_deep),
            [(
                "rules/0/any/0/all/0/all/0/all/0/all".into(),
                code::RULE_INVALID
            )],
            "the fifth group is the one too deep"
        );

        let wide = json!([{ "any": vec![exists.clone(); MAX_GROUP_RULES + 1] }]);
        assert_eq!(problems(wide), [("rules/0/any".into(), code::RULE_INVALID)]);
        assert_eq!(
            problems(json!([{ "all": [] }])),
            [("rules/0/all".into(), code::RULE_INVALID)]
        );
        let values: Vec<Value> = (0..=MAX_VALUES).map(|n| json!(n)).collect();
        assert_eq!(
            problems(json!([{ "attribute": "a", "op": "in", "value": values }])),
            [("rules/0/value".into(), code::RULE_INVALID)]
        );
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        assert_eq!(
            problems(json!([{ "attribute": long, "op": "exists" }])),
            [("rules/0/attribute".into(), code::RULE_INVALID)]
        );
    }

    #[test]
    fn every_problem_is_listed() {
        let found = problems(json!([
            { "attribute": "a", "op": "exists" },
            { "attribute": 1, "op": "nope" },
            { "any": [{ "attribute": "b", "op": "equals", "value": 2.5 }] }
        ]));
        assert_eq!(
            found,
            [
                ("rules/1/attribute".into(), code::RULE_INVALID),
                ("rules/1/op".into(), code::RULE_INVALID),
                ("rules/2/any/0/value".into(), code::RULE_INVALID),
            ]
        );
    }

    #[test]
    fn a_name_the_provider_does_not_map_is_unknown() {
        let known: BTreeSet<String> = ["rating".to_owned()].into();
        let list = vec![
            json!({ "attribute": "rating", "op": "exists" }),
            json!({ "any": [{ "attribute": "ratign", "op": "exists" }] }),
        ];
        let found = parse_list(&list, &root(), Some(&known)).expect_err("refused");
        assert_eq!(
            found,
            [Problem {
                path: vec![
                    "rules".into(),
                    "1".into(),
                    "any".into(),
                    "0".into(),
                    "attribute".into()
                ],
                code: code::ATTRIBUTE_UNKNOWN,
            }]
        );
        assert!(parse_list(&list[..1], &root(), Some(&known)).is_ok());
    }
}
