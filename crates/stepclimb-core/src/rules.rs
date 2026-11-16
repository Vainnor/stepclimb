// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rule data of modules and items (docs/specs/m1/04-unlock-and-completion-rules.md B1 to
//! B3): a module's completion rule, an item's typed [`Requirement`], and the prerequisite graph's
//! cycle check. Plain values and functions; `modules::set_rules` and `items::set_requirement`
//! store them under the course lock.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::items::Kind;

/// The most prerequisites one module may name (B2).
pub const MAX_PREREQUISITES: usize = 20;

/// Whether all or one of a module's requirements complete it (B1, Q1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompletionRule {
    #[default]
    All,
    One,
}

impl CompletionRule {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::One => "one",
        }
    }

    /// The rule stored as `value`, or `None` for a value the table's check refuses.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "one" => Some(Self::One),
            _ => None,
        }
    }
}

/// What a learner must do to complete an item (B3). `submit`, `score` and `mastery` are
/// reserved names: [`Requirement::parse`] refuses them until M2 and M3 define them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Requirement {
    /// Open the item.
    View,
    /// Mark the item done.
    MarkDone,
    /// Watch at least `min_percent` (1 to 100) of a video.
    Watch { min_percent: u8 },
}

/// Why a requirement was refused (B3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequirementError {
    /// Not an object of a known kind with exactly its members, or a kind the item does not
    /// take: `requirement.invalid`.
    Invalid,
    /// A kind reserved for M2 or M3: `requirement.unavailable`.
    Unavailable,
}

impl Requirement {
    /// The requirement `value` describes, checked member by member: an unknown member, a
    /// missing or out-of-range `min_percent`, or an unknown kind is
    /// [`RequirementError::Invalid`]; `submit`, `score` and `mastery` are
    /// [`RequirementError::Unavailable`] whatever their members.
    pub fn parse(value: &Value) -> Result<Self, RequirementError> {
        let object = value.as_object().ok_or(RequirementError::Invalid)?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(RequirementError::Invalid)?;
        let only = |members: &[&str]| object.keys().all(|key| members.contains(&key.as_str()));
        match kind {
            "view" if only(&["kind"]) => Ok(Self::View),
            "mark_done" if only(&["kind"]) => Ok(Self::MarkDone),
            "watch" if only(&["kind", "min_percent"]) => watch(object),
            "submit" | "score" | "mastery" => Err(RequirementError::Unavailable),
            _ => Err(RequirementError::Invalid),
        }
    }

    /// The stored and API form.
    #[must_use]
    pub fn to_json(self) -> Value {
        match self {
            Self::View => json!({ "kind": "view" }),
            Self::MarkDone => json!({ "kind": "mark_done" }),
            Self::Watch { min_percent } => json!({ "kind": "watch", "min_percent": min_percent }),
        }
    }

    /// Whether an item of `kind` takes the requirement (B3's table). A header takes none.
    #[must_use]
    pub fn applies_to(self, kind: Kind) -> bool {
        match self {
            Self::View | Self::MarkDone => matches!(
                kind,
                Kind::Page | Kind::File | Kind::ExternalLink | Kind::Video
            ),
            Self::Watch { .. } => kind == Kind::Video,
        }
    }
}

fn watch(object: &Map<String, Value>) -> Result<Requirement, RequirementError> {
    let min_percent = object
        .get("min_percent")
        .and_then(Value::as_u64)
        .filter(|percent| (1..=100).contains(percent))
        .and_then(|percent| u8::try_from(percent).ok())
        .ok_or(RequirementError::Invalid)?;
    Ok(Requirement::Watch { min_percent })
}

/// Whether giving `module` the prerequisites `requires` closes a cycle in the course's graph,
/// whose other edges are `edges` as `(module, requires)` pairs: true when `module` can be
/// reached from one of `requires` by following prerequisites (B2, Q2). `edges` must not hold
/// `module`'s own current pairs, which the write replaces.
#[must_use]
pub fn closes_cycle(edges: &[(Uuid, Uuid)], module: Uuid, requires: &[Uuid]) -> bool {
    let mut next: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for &(from, to) in edges {
        next.entry(from).or_default().push(to);
    }
    let mut seen = HashSet::new();
    let mut stack: Vec<Uuid> = requires.to_vec();
    while let Some(current) = stack.pop() {
        if current == module {
            return true;
        }
        if seen.insert(current) {
            stack.extend(next.get(&current).into_iter().flatten().copied());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::new_id;

    #[test]
    fn completion_rules_round_trip_their_names() {
        for rule in [CompletionRule::All, CompletionRule::One] {
            assert_eq!(CompletionRule::parse(rule.as_str()), Some(rule));
        }
        assert_eq!(CompletionRule::default(), CompletionRule::All);
        assert_eq!(CompletionRule::parse("any"), None);
    }

    #[test]
    fn t10_requirement_shapes() {
        let ok = [
            (json!({ "kind": "view" }), Requirement::View),
            (json!({ "kind": "mark_done" }), Requirement::MarkDone),
            (
                json!({ "kind": "watch", "min_percent": 1 }),
                Requirement::Watch { min_percent: 1 },
            ),
            (
                json!({ "kind": "watch", "min_percent": 100 }),
                Requirement::Watch { min_percent: 100 },
            ),
        ];
        for (value, requirement) in ok {
            assert_eq!(Requirement::parse(&value), Ok(requirement), "{value}");
            assert_eq!(requirement.to_json(), value);
        }
        let invalid = [
            json!({ "kind": "watch", "min_percent": 0 }),
            json!({ "kind": "watch", "min_percent": 101 }),
            json!({ "kind": "watch", "min_percent": 50.5 }),
            json!({ "kind": "watch", "min_percent": "50" }),
            json!({ "kind": "watch" }),
            json!({ "kind": "view", "extra": true }),
            json!({ "kind": "mark_done", "min_percent": 5 }),
            json!({ "kind": "complete" }),
            json!({ "kind": 1 }),
            json!({}),
            json!("view"),
            json!([]),
        ];
        for value in invalid {
            assert_eq!(
                Requirement::parse(&value),
                Err(RequirementError::Invalid),
                "{value}"
            );
        }
        for value in [
            json!({ "kind": "submit" }),
            json!({ "kind": "score", "min": 80 }),
            json!({ "kind": "mastery" }),
        ] {
            assert_eq!(
                Requirement::parse(&value),
                Err(RequirementError::Unavailable),
                "{value}"
            );
        }
    }

    #[test]
    fn t10_which_item_kinds_take_which_requirement() {
        use Kind::{Activity, ExternalLink, File, Header, Page, Quiz, Video};
        let watch = Requirement::Watch { min_percent: 50 };
        for kind in [Page, File, ExternalLink, Video] {
            assert!(Requirement::View.applies_to(kind), "{kind:?}");
            assert!(Requirement::MarkDone.applies_to(kind), "{kind:?}");
        }
        for kind in [Header, Quiz, Activity] {
            assert!(!Requirement::View.applies_to(kind), "{kind:?}");
            assert!(!Requirement::MarkDone.applies_to(kind), "{kind:?}");
        }
        assert!(watch.applies_to(Video));
        for kind in [Header, Page, File, ExternalLink, Quiz, Activity] {
            assert!(!watch.applies_to(kind), "{kind:?}");
        }
    }

    #[test]
    fn t9_a_cycle_is_found_through_any_path() {
        let (a, b, c, d) = (new_id(), new_id(), new_id(), new_id());
        // B requires A: A requiring B closes A -> B -> A.
        assert!(closes_cycle(&[(b, a)], a, &[b]));
        // C requires B requires A: A requiring C closes a three-module cycle.
        assert!(closes_cycle(&[(c, b), (b, a)], a, &[c]));
        // A module requiring itself.
        assert!(closes_cycle(&[], a, &[a]));
        // Diamonds and chains without a way back are fine.
        assert!(!closes_cycle(&[(c, a), (c, b), (d, c)], a, &[b]));
        assert!(!closes_cycle(&[(b, a)], c, &[a, b]));
        assert!(!closes_cycle(&[], a, &[]));
        // A cycle elsewhere in the graph does not make every write a cycle, and the walk ends.
        assert!(!closes_cycle(&[(b, c), (c, b)], a, &[b]));
    }
}
