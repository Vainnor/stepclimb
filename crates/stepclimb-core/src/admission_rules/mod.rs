// SPDX-License-Identifier: AGPL-3.0-or-later
//! Admission rules: declarative conditions on a sign-in's mapped attributes
//! (docs/specs/m0/07-admission.md B8 to B10, Q1 (b), Q2 (a), Q3 (a)).
//!
//! An organization has one organization-wide list, stored in `organizations.settings` as
//! `admission.rules`, and one list per outside provider, stored in that provider's
//! `identity_providers.config` as `admission_rules` (Q2 (a): attribute names differ between
//! providers). An empty list is stored as no member. The rule model and its JSON form are
//! [`model`]'s; evaluation is [`eval`]'s.
//!
//! A sign-in through an outside provider must pass every rule of the organization's list and
//! of that provider's list ([`SignInRules::refusal`]). Writes go through [`validate`], which
//! checks a change against the lists read under [`lock`] and the attributes each provider
//! maps, and [`update`], which writes the lists that differ with one audit entry each. Every
//! query filters by the scope's organization.

mod eval;
mod model;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::SystemTime;

use serde_json::{Value, json};
use sqlx::{PgConnection, PgExecutor};

pub use eval::{Outcome, evaluate};
pub use model::{
    Condition, MAX_DEPTH, MAX_GROUP_RULES, MAX_RULES, MAX_TEXT_CHARS, MAX_VALUES, Pattern, Problem,
    Rule, Scalar, Test, attribute_names, code, parse_list, to_json,
};

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    identity_providers::{self, IdentityProvider},
};

/// The member of a provider's `config` that holds its list.
pub const PROVIDER_MEMBER: &str = "admission_rules";

/// Why an admission rules repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    NotFound,
    /// A stored list does not parse: corruption or a newer release's form. Admission refuses
    /// every sign-in it would apply to.
    #[error("the stored admission rules at {0} do not parse")]
    StoredRules(String),
    #[error(transparent)]
    Provider(#[from] identity_providers::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// A stored list that does not parse, by where it is stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable(pub String);

/// A stored list as read: its rules, or [`Unreadable`].
pub type Stored = Result<Vec<Rule>, Unreadable>;

/// One provider and its list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRules {
    pub provider: IdentityProvider,
    pub rules: Stored,
}

/// Every list of an organization: its own and each outside provider's, by key. [`lock`] reads
/// a list that does not parse as [`Unreadable`], so a change can replace it; [`get`] refuses
/// one ([`RuleSets::check`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSets {
    pub organization: Stored,
    /// Every outside provider, enabled or not, sorted by key; an empty list for one with none.
    pub providers: Vec<ProviderRules>,
}

impl RuleSets {
    /// [`Error::StoredRules`] naming the first list that does not parse, if any.
    pub fn check(&self) -> Result<(), Error> {
        let lists =
            std::iter::once(&self.organization).chain(self.providers.iter().map(|p| &p.rules));
        for list in lists {
            if let Err(Unreadable(location)) = list {
                return Err(Error::StoredRules(location.clone()));
            }
        }
        Ok(())
    }
}

/// Which rule refused a sign-in: its JSON Pointer in the admin API's `rules`, such as
/// `/rules/organization/0` or `/rules/providers/sso/1`. It names no attribute value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleId {
    Organization(usize),
    Provider { key: String, index: usize },
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Organization(index) => write!(f, "/rules/organization/{index}"),
            Self::Provider { key, index } => {
                let key = key.replace('~', "~0").replace('/', "~1");
                write!(f, "/rules/providers/{key}/{index}")
            }
        }
    }
}

/// The lists a sign-in through one provider must pass.
#[derive(Clone, Debug)]
pub struct SignInRules {
    organization: Vec<Rule>,
    provider_key: String,
    provider: Vec<Rule>,
}

impl SignInRules {
    /// The first rule, the organization's list first and in list order, that does not hold for
    /// `attributes`, with what it said; `None` when every rule holds, as with no rules.
    #[must_use]
    pub fn refusal(
        &self,
        attributes: &serde_json::Map<String, Value>,
    ) -> Option<(RuleId, Outcome)> {
        let organization = self
            .organization
            .iter()
            .enumerate()
            .map(|(index, rule)| (RuleId::Organization(index), rule));
        let provider = self.provider.iter().enumerate().map(|(index, rule)| {
            let key = self.provider_key.clone();
            (RuleId::Provider { key, index }, rule)
        });
        organization
            .chain(provider)
            .map(|(id, rule)| (id, evaluate(rule, attributes)))
            .find(|(_, outcome)| *outcome != Outcome::Holds)
    }
}

/// The lists a sign-in through `provider`, a provider of the scope's organization as read in
/// the sign-in's transaction, must pass.
pub async fn for_sign_in(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    provider: &IdentityProvider,
) -> Result<SignInRules, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT (settings -> 'admission' -> 'rules')::text AS rules FROM organizations
           WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(SignInRules {
        organization: organization_rules(stored.as_deref()).map_err(unreadable)?,
        provider_key: provider.key.clone(),
        provider: provider_rules(provider).map_err(unreadable)?,
    })
}

/// The scope's organization's own list, [`Unreadable`] when it does not parse. Read under the
/// organization row's lock (the lock every rule write takes first), it holds until the caller's
/// transaction ends.
pub async fn organization_list(conn: &mut PgConnection, scope: OrgScope) -> Result<Stored, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT (settings -> 'admission' -> 'rules')::text AS rules FROM organizations
           WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(organization_rules(stored.as_deref()))
}

/// Every list of the scope's organization; [`Error::StoredRules`] when one does not parse.
pub async fn get(conn: &mut PgConnection, scope: OrgScope) -> Result<RuleSets, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT (settings -> 'admission' -> 'rules')::text AS rules FROM organizations
           WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    let providers = identity_providers::list_outside(&mut *conn, scope).await?;
    let sets = rule_sets(stored.as_deref(), providers);
    sets.check()?;
    Ok(sets)
}

/// [`get`], locking the organization's row (the lock every settings write takes) and then
/// every outside provider's row until the caller's transaction ends, so [`update`] in the same
/// transaction replaces the lists read here. A list that does not parse is read as
/// [`Unreadable`] rather than refused, so a change naming it can replace it.
pub async fn lock(conn: &mut PgConnection, scope: OrgScope) -> Result<RuleSets, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT (settings -> 'admission' -> 'rules')::text AS rules FROM organizations
           WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    let providers = identity_providers::lock_outside(&mut *conn, scope).await?;
    Ok(rule_sets(stored.as_deref(), providers))
}

fn rule_sets(organization: Option<&str>, providers: Vec<IdentityProvider>) -> RuleSets {
    let providers = providers
        .into_iter()
        .map(|provider| {
            let rules = provider_rules(&provider);
            ProviderRules { provider, rules }
        })
        .collect();
    RuleSets {
        organization: organization_rules(organization),
        providers,
    }
}

fn unreadable(Unreadable(location): Unreadable) -> Error {
    Error::StoredRules(location)
}

/// The organization's list from its stored JSON text, `None` when there is none.
fn organization_rules(stored: Option<&str>) -> Stored {
    let location = || Unreadable("organizations.settings/admission/rules".into());
    let Some(stored) = stored else {
        return Ok(Vec::new());
    };
    let value: Value = serde_json::from_str(stored).map_err(|_| location())?;
    stored_list(Some(&value)).ok_or_else(location)
}

/// `provider`'s list from its config, [`Unreadable`] when it does not parse.
pub fn provider_rules(provider: &IdentityProvider) -> Stored {
    stored_list(provider.config.get(PROVIDER_MEMBER)).ok_or_else(|| {
        Unreadable(format!(
            "identity_providers({})/{PROVIDER_MEMBER}",
            provider.key
        ))
    })
}

/// A stored list, empty when absent; `None` when it is not a list of valid rules. A stored
/// list's attribute names are not checked: a provider may stop mapping one, and the rule then
/// finds the attribute absent and fails.
fn stored_list(value: Option<&Value>) -> Option<Vec<Rule>> {
    match value {
        None => Some(Vec::new()),
        Some(Value::Array(rules)) => parse_list(rules, &[], None).ok(),
        Some(_) => None,
    }
}

/// A change an admin asks for: `None` leaves a list as it is, and a provider the map does not
/// name keeps its list.
#[derive(Clone, Copy, Debug, Default)]
pub struct RulesInput<'a> {
    pub organization: Option<&'a [Value]>,
    pub providers: Option<&'a BTreeMap<String, Vec<Value>>>,
}

/// The lists a valid [`RulesInput`] changes: only the ones that differ from the current lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RulesChange {
    pub organization: Option<Vec<Rule>>,
    pub providers: BTreeMap<String, Vec<Rule>>,
}

impl RulesChange {
    /// Whether the change leaves every list as it is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.organization.is_none() && self.providers.is_empty()
    }
}

/// Check `input` against `current`, the lists [`lock`] read. `mapped` names the attributes a
/// provider maps (its config's `claims.attributes`, which this crate does not parse). A
/// provider's list may name only attributes that provider maps, and the organization's list
/// only attributes some outside provider maps ([`code::ATTRIBUTE_UNKNOWN`]); a provider key
/// that is not an outside provider of the organization is [`code::PROVIDER_UNKNOWN`]. Problem
/// paths start at `rules`, as the admin API's body names it.
pub fn validate(
    current: &RuleSets,
    input: RulesInput<'_>,
    mapped: impl Fn(&IdentityProvider) -> BTreeSet<String>,
) -> Result<RulesChange, Vec<Problem>> {
    let by_key: BTreeMap<&str, &ProviderRules> = current
        .providers
        .iter()
        .map(|p| (p.provider.key.as_str(), p))
        .collect();
    let mut problems = Vec::new();
    let mut change = RulesChange::default();
    if let Some(rules) = input.organization {
        let known: BTreeSet<String> = current
            .providers
            .iter()
            .flat_map(|p| mapped(&p.provider))
            .collect();
        let path = ["rules".to_owned(), "organization".to_owned()];
        match parse_list(rules, &path, Some(&known)) {
            Ok(rules) if current.organization.as_ref() != Ok(&rules) => {
                change.organization = Some(rules);
            }
            Ok(_) => {}
            Err(found) => problems.extend(found),
        }
    }
    for (key, rules) in input.providers.into_iter().flatten() {
        let path = ["rules".to_owned(), "providers".to_owned(), key.clone()];
        let Some(stored) = by_key.get(key.as_str()) else {
            problems.push(Problem {
                path: path.to_vec(),
                code: code::PROVIDER_UNKNOWN,
            });
            continue;
        };
        let known = mapped(&stored.provider);
        match parse_list(rules, &path, Some(&known)) {
            Ok(rules) if stored.rules.as_ref() != Ok(&rules) => {
                change.providers.insert(key.clone(), rules);
            }
            Ok(_) => {}
            Err(found) => problems.extend(found),
        }
    }
    if problems.is_empty() {
        Ok(change)
    } else {
        Err(problems)
    }
}

/// Write `change` over `current`, the lists [`lock`] read on the same connection, by `actor`
/// at `now`, and return the lists after it. Each list that changes gets its own audit entry
/// (spec 09 B22): `organization.update_settings` with `admission.rules` before and after, or
/// `identity_provider.update` with the provider's `admission_rules` before and after; `before`
/// is `null` for a list that did not parse. An empty change writes nothing.
pub async fn update(
    conn: &mut PgConnection,
    scope: OrgScope,
    mut current: RuleSets,
    change: RulesChange,
    now: SystemTime,
    actor: Actor,
) -> Result<RuleSets, Error> {
    let now = epoch_micros(now)?;
    if let Some(rules) = change.organization {
        let (before, after) = (stored_json(&current.organization), to_json(&rules));
        sqlx::query!(
            "UPDATE organizations
             SET settings = CASE WHEN $2::text::jsonb = '[]'::jsonb
                     THEN settings #- '{admission,rules}'
                     ELSE settings || jsonb_build_object('admission',
                          COALESCE(settings -> 'admission', '{}'::jsonb)
                          || jsonb_build_object('rules', $2::text::jsonb))
                 END,
                 updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
             WHERE id = $1",
            scope.org_id(),
            after.to_string(),
            now,
        )
        .execute(&mut *conn)
        .await?;
        let data = json!({
            "before": { "admission": { "rules": before } },
            "after": { "admission": { "rules": after } },
        });
        let action = Action::OrganizationUpdateSettings;
        audit::record(conn, scope, actor, action, scope.org_id(), &data, now).await?;
        current.organization = Ok(rules);
    }
    for stored in &mut current.providers {
        let Some(rules) = change.providers.get(&stored.provider.key) else {
            continue;
        };
        write_provider(conn, scope, stored, rules, now, actor).await?;
        stored.rules = Ok(rules.clone());
    }
    Ok(current)
}

/// A list's `before` in an audit entry: the rules, or `null` for a list that did not parse.
fn stored_json(stored: &Stored) -> Value {
    stored.as_ref().map_or(Value::Null, |rules| to_json(rules))
}

/// Replace `stored`'s list with `rules`, with its audit entry.
async fn write_provider(
    conn: &mut PgConnection,
    scope: OrgScope,
    stored: &ProviderRules,
    rules: &[Rule],
    now: i64,
    actor: Actor,
) -> Result<(), Error> {
    let (before, after) = (stored_json(&stored.rules), to_json(rules));
    let id = stored.provider.id;
    sqlx::query!(
        "UPDATE identity_providers
         SET config = CASE WHEN $3::text::jsonb = '[]'::jsonb
                 THEN config - 'admission_rules'
                 ELSE config || jsonb_build_object('admission_rules', $3::text::jsonb)
             END,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        after.to_string(),
        now,
    )
    .execute(&mut *conn)
    .await?;
    let data = json!({
        "before": { PROVIDER_MEMBER: before },
        "after": { PROVIDER_MEMBER: after },
    });
    let action = Action::IdentityProviderUpdate;
    audit::record(conn, scope, actor, action, id, &data, now).await?;
    Ok(())
}

/// Record that step 2 refused the user `user_id`'s sign-in through the provider `provider_key`
/// at `now` (spec 07 I3, the refusal marker). A local sign-in refuses a user with the marker
/// ([`refused_by`]); the next sign-in through an outside provider that passes the rules clears
/// it ([`clear_refused`]). It is not a profile change: `updated_at` stays.
pub async fn mark_refused(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: uuid::Uuid,
    provider_key: &str,
    now: SystemTime,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    sqlx::query!(
        "UPDATE users
         SET admission_refused_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
             admission_refused_provider = $3
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        user_id,
        provider_key,
        now,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Clear the user `user_id`'s refusal marker, if it has one.
pub async fn clear_refused(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: uuid::Uuid,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE users SET admission_refused_at = NULL, admission_refused_provider = NULL
         WHERE org_id = $1 AND id = $2 AND admission_refused_at IS NOT NULL",
        scope.org_id(),
        user_id,
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The provider key of the user `user_id`'s refusal marker, or `None` when the user has none
/// (or the organization has no such user).
pub async fn refused_by(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: uuid::Uuid,
) -> Result<Option<String>, Error> {
    let provider = sqlx::query_scalar!(
        "SELECT admission_refused_provider FROM users WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        user_id,
    )
    .fetch_optional(db)
    .await?
    .flatten();
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    fn rules(value: Value) -> Vec<Rule> {
        stored_list(Some(&value)).expect("valid rules")
    }

    fn provider(key: &str, config: Value) -> IdentityProvider {
        let Value::Object(config) = config else {
            panic!("an object")
        };
        IdentityProvider {
            id: uuid::Uuid::nil(),
            key: key.into(),
            kind: identity_providers::Kind::Oidc,
            config,
            enabled: true,
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn attrs(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("an object"),
        }
    }

    #[test]
    fn a_rule_id_is_its_pointer_in_the_admin_api() {
        assert_eq!(RuleId::Organization(2).to_string(), "/rules/organization/2");
        let id = RuleId::Provider {
            key: "a/b~c".into(),
            index: 0,
        };
        assert_eq!(id.to_string(), "/rules/providers/a~1b~0c/0");
    }

    #[test]
    fn the_refusal_is_the_first_rule_that_does_not_hold() {
        let sign_in = SignInRules {
            organization: rules(json!([
                { "attribute": "member", "op": "exists" },
                { "attribute": "subdivision", "op": "equals", "value": "USA" }
            ])),
            provider_key: "sso".into(),
            provider: rules(json!([{ "attribute": "rating", "op": "in", "value": ["S1", "S2"] }])),
        };
        let pass = attrs(json!({ "member": 1, "subdivision": "USA", "rating": "S2" }));
        assert_eq!(sign_in.refusal(&pass), None);
        let other = attrs(json!({ "member": 1, "subdivision": "CAN", "rating": 2 }));
        assert_eq!(
            sign_in.refusal(&other),
            Some((RuleId::Organization(1), Outcome::Fails))
        );
        let wrong_type = attrs(json!({ "member": 1, "subdivision": "USA", "rating": 2 }));
        assert_eq!(
            sign_in.refusal(&wrong_type),
            Some((
                RuleId::Provider {
                    key: "sso".into(),
                    index: 0
                },
                Outcome::Unevaluable
            ))
        );
        let none = SignInRules {
            organization: Vec::new(),
            provider_key: "sso".into(),
            provider: Vec::new(),
        };
        assert_eq!(none.refusal(&Map::new()), None, "no rules admit");
    }

    #[test]
    fn a_stored_list_that_does_not_parse_is_an_error() {
        assert_eq!(organization_rules(None), Ok(Vec::new()));
        assert!(organization_rules(Some("{}")).is_err());
        assert!(organization_rules(Some(r#"[{"op": "exists"}]"#)).is_err());
        assert!(organization_rules(Some("not json")).is_err());
        let broken = provider("sso", json!({ PROVIDER_MEMBER: [{ "any": [] }] }));
        assert!(matches!(
            provider_rules(&broken),
            Err(Unreadable(at)) if at == "identity_providers(sso)/admission_rules"
        ));
        assert_eq!(provider_rules(&provider("sso", json!({}))), Ok(Vec::new()));
    }

    fn sets() -> RuleSets {
        RuleSets {
            organization: Ok(rules(json!([{ "attribute": "rating", "op": "exists" }]))),
            providers: vec![
                ProviderRules {
                    provider: provider("a", json!({})),
                    rules: Ok(Vec::new()),
                },
                ProviderRules {
                    provider: provider("b", json!({})),
                    rules: Ok(rules(json!([{ "attribute": "region", "op": "exists" }]))),
                },
            ],
        }
    }

    /// Provider `a` maps `rating`, `b` maps `region`.
    fn mapped(provider: &IdentityProvider) -> BTreeSet<String> {
        let name = if provider.key == "a" {
            "rating"
        } else {
            "region"
        };
        [name.to_owned()].into()
    }

    fn problem_paths(found: Vec<Problem>) -> Vec<(String, &'static str)> {
        found
            .into_iter()
            .map(|p| (p.path.join("/"), p.code))
            .collect()
    }

    #[test]
    fn a_list_that_does_not_parse_can_be_replaced_and_fails_the_check() {
        let mut broken = sets();
        broken.providers[1].rules = Err(Unreadable("identity_providers(b)/admission_rules".into()));
        assert!(matches!(broken.check(), Err(Error::StoredRules(at)) if at.contains("(b)")));
        let same = vec![json!({ "attribute": "region", "op": "exists" })];
        let providers: BTreeMap<String, Vec<Value>> = [("b".to_owned(), same)].into();
        let input = RulesInput {
            organization: None,
            providers: Some(&providers),
        };
        let change = validate(&broken, input, mapped).expect("valid");
        assert_eq!(
            change.providers.keys().collect::<Vec<_>>(),
            ["b"],
            "an unreadable list differs from any rules"
        );
        assert_eq!(stored_json(&broken.providers[1].rules), Value::Null);
        assert!(sets().check().is_ok());
    }

    #[test]
    fn a_change_names_only_the_lists_that_differ() {
        let same = vec![json!({ "attribute": "rating", "op": "exists" })];
        let providers: BTreeMap<String, Vec<Value>> = [
            (
                "a".to_owned(),
                vec![json!({ "attribute": "rating", "op": "exists" })],
            ),
            (
                "b".to_owned(),
                vec![json!({ "attribute": "region", "op": "exists" })],
            ),
        ]
        .into();
        let input = RulesInput {
            organization: Some(&same),
            providers: Some(&providers),
        };
        let change = validate(&sets(), input, mapped).expect("valid");
        assert_eq!(change.organization, None);
        assert_eq!(
            change.providers.keys().collect::<Vec<_>>(),
            ["a"],
            "b's list is unchanged"
        );
        assert!(
            validate(&sets(), RulesInput::default(), mapped)
                .expect("valid")
                .is_empty()
        );
    }

    #[test]
    fn the_organizations_list_takes_any_providers_attributes() {
        let both = vec![
            json!({ "attribute": "rating", "op": "exists" }),
            json!({ "attribute": "region", "op": "exists" }),
            json!({ "attribute": "rank", "op": "exists" }),
        ];
        let input = RulesInput {
            organization: Some(&both),
            providers: None,
        };
        let found = validate(&sets(), input, mapped).expect_err("refused");
        assert_eq!(
            problem_paths(found),
            [(
                "rules/organization/2/attribute".into(),
                code::ATTRIBUTE_UNKNOWN
            )]
        );
    }

    #[test]
    fn a_providers_list_takes_only_its_own_attributes_and_known_keys() {
        let providers: BTreeMap<String, Vec<Value>> = [
            (
                "a".to_owned(),
                vec![json!({ "attribute": "region", "op": "exists" })],
            ),
            ("local".to_owned(), Vec::new()),
            ("zz".to_owned(), Vec::new()),
        ]
        .into();
        let input = RulesInput {
            organization: None,
            providers: Some(&providers),
        };
        let found = validate(&sets(), input, mapped).expect_err("refused");
        assert_eq!(
            problem_paths(found),
            [
                (
                    "rules/providers/a/0/attribute".into(),
                    code::ATTRIBUTE_UNKNOWN
                ),
                ("rules/providers/local".into(), code::PROVIDER_UNKNOWN),
                ("rules/providers/zz".into(), code::PROVIDER_UNKNOWN),
            ]
        );
    }
}
