// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission evaluator (docs/specs/m0/09-permissions-and-roles.md B3, B9, B16 to B18).
//!
//! [`Grants::load`] reads a user's role assignments in one query and turns them into grants:
//! the permissions effective at the organization, and those effective at each account. A
//! permission is effective at an assignment's scope only when the catalog lists it and its
//! [`Permission::GRANTABLE`] includes that scope type (B3); stored keys the catalog does not
//! know are left out and reported by [`Grants::ignored_keys`] (B9). Course-scope assignments
//! grant nothing until the M1 courses spec adds course permissions and `CourseTarget`.
//!
//! Scopes nest. A grant at the organization contains every account; a grant at account A
//! contains A and every account whose path runs through A. The checks take the target's path
//! ([`Target`]), so "does a grant contain the target" is "is the grant's account on the
//! target's path".
//!
//! A [`Grants`] is built for one request and never cached across requests, so a removed
//! assignment or a changed custom role takes effect on the next request (B16).

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope, accounts,
    perm::{self, Permission, ScopeType},
    role_assignments::{self, GrantScope, UserGrant},
};

/// Where a checked target sits in the organization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The organization itself.
    Organization,
    /// An account, by the ids on its path: the root first, the account itself last.
    Account(Vec<Uuid>),
}

/// A scope named by id: what [`Grants::scopes_with`] lists and what [`Grants::require_at`]
/// checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeId {
    /// An organization by id. [`Grants::require_at`] finds only the grants' own.
    Organization(Uuid),
    /// One account of that organization, with its subtree.
    Account(Uuid),
}

/// Why a check refused (B17 steps 3 to 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    /// The account does not exist in the organization, or the principal holds nothing at a
    /// scope that contains it. The two answer the same, so another scope's ids cannot be told
    /// apart from ids that do not exist.
    #[error("the account does not exist")]
    AccountNotFound,
    /// The organization named is not the principal's.
    #[error("the organization does not exist")]
    OrganizationNotFound,
    /// The target is visible but the permission is not effective at a scope containing it.
    #[error("the permission is not held at this scope")]
    Forbidden,
}

/// Why [`Grants::require_at`] failed.
#[derive(Debug, thiserror::Error)]
pub enum RequireError {
    #[error(transparent)]
    Denied(#[from] Denied),
    /// Reading the target's path failed.
    #[error(transparent)]
    Account(#[from] accounts::Error),
}

/// A principal's effective permissions in one organization, per scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grants {
    scope: OrgScope,
    organization: BTreeSet<&'static str>,
    /// Only accounts where at least one permission is effective.
    accounts: BTreeMap<Uuid, BTreeSet<&'static str>>,
    ignored: BTreeSet<String>,
}

impl Grants {
    /// The grants of the user `user_id` in the scope's organization, from one query (B16).
    /// A user the organization does not have holds nothing.
    pub async fn load(
        db: impl PgExecutor<'_>,
        scope: OrgScope,
        user_id: Uuid,
    ) -> Result<Self, role_assignments::Error> {
        let assignments = role_assignments::for_user_with_paths(db, scope, user_id).await?;
        Ok(Self::from_assignments(scope, assignments))
    }

    /// The grants a user's assignments in the scope's organization give, as
    /// [`role_assignments::for_user_with_paths`] reads them.
    #[must_use]
    pub fn from_assignments(
        scope: OrgScope,
        assignments: impl IntoIterator<Item = UserGrant>,
    ) -> Self {
        let mut grants = Self {
            scope,
            organization: BTreeSet::new(),
            accounts: BTreeMap::new(),
            ignored: BTreeSet::new(),
        };
        for assignment in assignments {
            // `None` for a course scope, which grants nothing until M1; its unknown keys are
            // still reported.
            let placement = match assignment.scope {
                GrantScope::Organization => Some((ScopeType::Organization, None)),
                GrantScope::Account { id, .. } => Some((ScopeType::Account, Some(id))),
                GrantScope::Course(_) => None,
            };
            let mut effective = BTreeSet::new();
            for key in assignment.permissions {
                let Some(p) = perm::ALL.iter().find(|p| p.key() == key) else {
                    grants.ignored.insert(key);
                    continue;
                };
                if let Some((scope_type, _)) = placement
                    && p.grantable().contains(&scope_type)
                {
                    effective.insert(p.key());
                }
            }
            if let Some((_, account)) = placement {
                grants.add(account, effective);
            }
        }
        grants
    }

    fn add(&mut self, account: Option<Uuid>, effective: BTreeSet<&'static str>) {
        if effective.is_empty() {
            return;
        }
        match account {
            None => self.organization.extend(effective),
            Some(id) => self.accounts.entry(id).or_default().extend(effective),
        }
    }

    /// Keep only the permissions an API key's scopes name: the key's rights are its scopes
    /// intersected with its owner's current grants (B16, spec 08 B27). A scope string that is
    /// not a catalog key matches nothing.
    pub fn intersect_key_scopes<S: AsRef<str>>(&mut self, key_scopes: &[S]) {
        let named = |key: &&str| key_scopes.iter().any(|s| s.as_ref() == *key);
        self.organization.retain(named);
        for keys in self.accounts.values_mut() {
            keys.retain(named);
        }
        self.accounts.retain(|_, keys| !keys.is_empty());
    }

    /// The organization these grants are in.
    #[must_use]
    pub fn scope(&self) -> OrgScope {
        self.scope
    }

    /// The stored permission keys the catalog does not know, which grant nothing (B9).
    pub fn ignored_keys(&self) -> impl Iterator<Item = &str> {
        self.ignored.iter().map(String::as_str)
    }

    /// The permissions of each account grant on `path`, that is each account grant containing
    /// the account the path ends at.
    fn account_keys_on<'a>(
        &'a self,
        path: &'a [Uuid],
    ) -> impl Iterator<Item = &'a BTreeSet<&'static str>> {
        path.iter().filter_map(|id| self.accounts.get(id))
    }

    /// Whether the principal may learn that the target exists: the organization always is
    /// visible to its members, and an account when any permission is effective at a scope
    /// containing it (B17 step 4).
    #[must_use]
    pub fn is_visible(&self, target: &Target) -> bool {
        match target {
            Target::Organization => true,
            Target::Account(path) => {
                !self.organization.is_empty() || self.account_keys_on(path).next().is_some()
            }
        }
    }

    /// Whether `P` is effective at a scope containing the target.
    #[must_use]
    pub fn holds<P: Permission>(&self, target: &Target) -> bool {
        self.holds_key(P::KEY, target)
    }

    /// Whether the permission with catalog key `key` is effective at a scope containing the
    /// target, for checks over a list of keys such as a role's (the escalation rule, B8 and
    /// B12). A key the catalog does not define is never effective.
    #[must_use]
    pub fn holds_key(&self, key: &str, target: &Target) -> bool {
        if self.organization.contains(key) {
            return true;
        }
        match target {
            Target::Organization => false,
            Target::Account(path) => self.account_keys_on(path).any(|keys| keys.contains(key)),
        }
    }

    /// Whether `P` is effective at any scope.
    #[must_use]
    pub fn holds_anywhere<P: Permission>(&self) -> bool {
        self.organization.contains(P::KEY) || self.accounts.values().any(|k| k.contains(P::KEY))
    }

    /// B17 steps 4 and 5 for a target that exists: not visible is
    /// [`Denied::AccountNotFound`], visible without `P` is [`Denied::Forbidden`].
    pub fn check<P: Permission>(&self, target: &Target) -> Result<(), Denied> {
        if !self.is_visible(target) {
            return Err(Denied::AccountNotFound);
        }
        if !self.holds::<P>(target) {
            return Err(Denied::Forbidden);
        }
        Ok(())
    }

    /// Every scope where `P` is effective, for a list filtered to what the principal may see
    /// (B18 `AnyScope`). Just the organization when `P` is effective there, since it contains
    /// every account; otherwise the accounts, sorted by id.
    #[must_use]
    pub fn scopes_with<P: Permission>(&self) -> Vec<ScopeId> {
        if self.organization.contains(P::KEY) {
            return vec![ScopeId::Organization(self.scope.org_id())];
        }
        self.accounts
            .iter()
            .filter(|(_, keys)| keys.contains(P::KEY))
            .map(|(id, _)| ScopeId::Account(*id))
            .collect()
    }

    /// B17 steps 3 to 5 for a target the request names in its body: an organization must be the
    /// grants' own ([`Denied::OrganizationNotFound`] otherwise) and an account must exist in it
    /// ([`Denied::AccountNotFound`] otherwise), then [`Self::check`]. Returns the target's path.
    pub async fn require_at<P: Permission>(
        &self,
        db: impl PgExecutor<'_>,
        at: ScopeId,
    ) -> Result<Target, RequireError> {
        let target = match at {
            ScopeId::Organization(id) if id == self.scope.org_id() => Target::Organization,
            ScopeId::Organization(_) => return Err(Denied::OrganizationNotFound.into()),
            ScopeId::Account(id) => Target::Account(
                accounts::ancestors_path(db, self.scope, id)
                    .await?
                    .ok_or(Denied::AccountNotFound)?,
            ),
        };
        self.check::<P>(&target)?;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        id::new_id,
        perm::{AccountManage, AccountView, OrgManage, RoleManage, UserView},
        roles,
    };

    /// The tree of spec 09 T2: Tower and Enroute under the root, Ground under Tower.
    struct Tree {
        root: Uuid,
        tower: Uuid,
        ground: Uuid,
        enroute: Uuid,
    }

    impl Tree {
        fn new() -> Self {
            Self {
                root: new_id(),
                tower: new_id(),
                ground: new_id(),
                enroute: new_id(),
            }
        }

        fn root(&self) -> Target {
            Target::Account(vec![self.root])
        }

        fn tower(&self) -> Target {
            Target::Account(vec![self.root, self.tower])
        }

        fn ground(&self) -> Target {
            Target::Account(vec![self.root, self.tower, self.ground])
        }

        fn enroute(&self) -> Target {
            Target::Account(vec![self.root, self.enroute])
        }

        fn at(&self, id: Uuid) -> GrantScope {
            let path = if id == self.root {
                vec![self.root]
            } else if id == self.ground {
                vec![self.root, self.tower, self.ground]
            } else {
                vec![self.root, id]
            };
            GrantScope::Account { id, path }
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "unit tests of the evaluator need a scope and have no credential"
    )]
    fn scope() -> OrgScope {
        OrgScope::instance(new_id())
    }

    fn assignment(permissions: &[&str], scope: GrantScope) -> UserGrant {
        UserGrant {
            assignment_id: new_id(),
            role_id: new_id(),
            role_key: "custom".into(),
            permissions: permissions.iter().map(|&k| k.to_owned()).collect(),
            scope,
        }
    }

    fn builtin(key: &str, scope: GrantScope) -> UserGrant {
        let role = roles::builtin(key).expect("a built-in role");
        assignment(&role.permissions(), scope)
    }

    fn grants(assignments: Vec<UserGrant>) -> Grants {
        Grants::from_assignments(scope(), assignments)
    }

    #[test]
    fn an_organization_grant_covers_every_account_and_the_organization() {
        let tree = Tree::new();
        let g = grants(vec![assignment(
            &["account.view"],
            GrantScope::Organization,
        )]);
        for target in [
            Target::Organization,
            tree.root(),
            tree.tower(),
            tree.ground(),
            tree.enroute(),
        ] {
            assert!(g.holds::<AccountView>(&target), "{target:?}");
            assert_eq!(g.check::<AccountView>(&target), Ok(()));
        }
        assert!(!g.holds::<AccountManage>(&tree.tower()));
    }

    #[test]
    fn an_account_grant_covers_its_subtree_only() {
        let tree = Tree::new();
        let g = grants(vec![assignment(&["account.view"], tree.at(tree.tower))]);
        assert!(g.holds::<AccountView>(&tree.tower()));
        assert!(g.holds::<AccountView>(&tree.ground()));
        assert!(!g.holds::<AccountView>(&tree.enroute()));
        assert!(!g.holds::<AccountView>(&tree.root()));
        assert!(!g.holds::<AccountView>(&Target::Organization));
    }

    #[test]
    fn holds_key_answers_as_holds_and_never_for_an_unknown_key() {
        let tree = Tree::new();
        let g = grants(vec![
            assignment(&["role.view"], GrantScope::Organization),
            assignment(&["account.manage", "retired.key"], tree.at(tree.tower)),
        ]);
        assert!(g.holds_key("role.view", &Target::Organization));
        assert!(g.holds_key("role.view", &tree.enroute()));
        assert!(g.holds_key("account.manage", &tree.ground()));
        assert!(!g.holds_key("account.manage", &tree.enroute()));
        assert!(!g.holds_key("account.manage", &Target::Organization));
        assert!(!g.holds_key("retired.key", &tree.tower()));
        assert!(!g.holds_key("", &Target::Organization));
    }

    #[test]
    fn org_admin_at_an_account_does_not_get_organization_only_permissions() {
        let tree = Tree::new();
        let g = grants(vec![builtin("org_admin", tree.at(tree.tower))]);
        assert!(!g.holds::<OrgManage>(&Target::Organization));
        assert!(!g.holds::<OrgManage>(&tree.tower()));
        assert!(!g.holds_anywhere::<OrgManage>());
        assert!(!g.holds_anywhere::<RoleManage>());
        assert!(g.holds::<AccountManage>(&tree.ground()));
        assert_eq!(
            g.check::<OrgManage>(&Target::Organization),
            Err(Denied::Forbidden)
        );
    }

    #[test]
    fn an_account_grant_of_user_view_grants_no_organization_target() {
        let tree = Tree::new();
        let g = grants(vec![assignment(&["user.view"], tree.at(tree.root))]);
        assert!(g.holds::<UserView>(&tree.root()));
        assert_eq!(
            g.check::<UserView>(&Target::Organization),
            Err(Denied::Forbidden)
        );
    }

    #[test]
    fn the_check_maps_invisible_to_not_found_and_visible_to_forbidden() {
        let tree = Tree::new();
        let none = grants(vec![]);
        assert_eq!(
            none.check::<AccountView>(&tree.tower()),
            Err(Denied::AccountNotFound)
        );
        let ground_only = grants(vec![assignment(&["account.view"], tree.at(tree.ground))]);
        assert_eq!(
            ground_only.check::<AccountView>(&tree.tower()),
            Err(Denied::AccountNotFound)
        );
        let other_at_root = grants(vec![assignment(&["role.view"], tree.at(tree.root))]);
        assert_eq!(
            other_at_root.check::<AccountView>(&tree.tower()),
            Err(Denied::Forbidden)
        );
        let other_at_org = grants(vec![assignment(&["role.view"], GrantScope::Organization)]);
        assert_eq!(
            other_at_org.check::<AccountView>(&tree.enroute()),
            Err(Denied::Forbidden)
        );
    }

    #[test]
    fn the_organization_is_visible_to_a_member_with_no_grants() {
        let none = grants(vec![]);
        assert!(none.is_visible(&Target::Organization));
        assert_eq!(
            none.check::<OrgManage>(&Target::Organization),
            Err(Denied::Forbidden)
        );
    }

    #[test]
    fn a_role_without_effective_permissions_makes_nothing_visible() {
        let tree = Tree::new();
        let g = grants(vec![
            builtin("student", tree.at(tree.tower)),
            // Grantable at organization scope only, so nothing at an account.
            assignment(&["organization.manage"], tree.at(tree.tower)),
        ]);
        assert!(!g.is_visible(&tree.tower()));
        assert_eq!(
            g.check::<AccountView>(&tree.tower()),
            Err(Denied::AccountNotFound)
        );
    }

    #[test]
    fn unknown_keys_grant_nothing_and_are_reported() {
        let tree = Tree::new();
        let g = grants(vec![assignment(
            &["account.view", "course.retired", "account.view"],
            tree.at(tree.tower),
        )]);
        assert!(g.holds::<AccountView>(&tree.tower()));
        assert_eq!(g.ignored_keys().collect::<Vec<_>>(), ["course.retired"]);
        let only_unknown = grants(vec![assignment(&["course.retired"], tree.at(tree.tower))]);
        assert!(!only_unknown.is_visible(&tree.tower()));
    }

    #[test]
    fn course_grants_grant_nothing() {
        let tree = Tree::new();
        let g = grants(vec![
            builtin("org_admin", GrantScope::Course(new_id())),
            assignment(
                &["account.view", "course.retired"],
                GrantScope::Course(new_id()),
            ),
        ]);
        assert!(!g.is_visible(&tree.root()));
        assert!(!g.holds_anywhere::<AccountView>());
        assert_eq!(g.scopes_with::<AccountView>(), []);
        // Unknown keys are reported on course rows too.
        assert_eq!(g.ignored_keys().collect::<Vec<_>>(), ["course.retired"]);
    }

    #[test]
    fn grants_at_several_scopes_combine() {
        let tree = Tree::new();
        let g = grants(vec![
            assignment(&["account.view"], tree.at(tree.tower)),
            assignment(&["account.manage"], tree.at(tree.ground)),
            assignment(&["role.view"], tree.at(tree.enroute)),
        ]);
        assert!(g.holds::<AccountManage>(&tree.ground()));
        assert!(g.holds::<AccountView>(&tree.ground()));
        assert!(!g.holds::<AccountManage>(&tree.tower()));
        assert_eq!(
            g.check::<AccountView>(&tree.enroute()),
            Err(Denied::Forbidden)
        );
        assert!(!g.is_visible(&tree.root()));
    }

    #[test]
    fn scopes_with_lists_the_organization_alone_or_each_account() {
        let tree = Tree::new();
        let accounts = grants(vec![
            assignment(&["account.view"], tree.at(tree.tower)),
            assignment(&["account.view", "role.view"], tree.at(tree.enroute)),
            assignment(&["role.view"], tree.at(tree.ground)),
        ]);
        let mut expected = vec![ScopeId::Account(tree.tower), ScopeId::Account(tree.enroute)];
        expected.sort();
        assert_eq!(accounts.scopes_with::<AccountView>(), expected);
        assert!(accounts.holds_anywhere::<AccountView>());
        assert!(!accounts.holds_anywhere::<AccountManage>());

        let org = grants(vec![
            assignment(&["account.view"], GrantScope::Organization),
            assignment(&["account.view"], tree.at(tree.tower)),
        ]);
        assert_eq!(
            org.scopes_with::<AccountView>(),
            [ScopeId::Organization(org.scope().org_id())]
        );
    }

    #[test]
    fn an_api_key_keeps_only_the_permissions_its_scopes_name() {
        let tree = Tree::new();
        let mut g = grants(vec![
            builtin("account_admin", tree.at(tree.tower)),
            assignment(&["role.manage"], GrantScope::Organization),
        ]);
        g.intersect_key_scopes(&["account.view", "not.a_permission"]);
        assert!(g.holds::<AccountView>(&tree.ground()));
        assert_eq!(
            g.check::<AccountManage>(&tree.tower()),
            Err(Denied::Forbidden)
        );
        assert!(!g.holds_anywhere::<RoleManage>());
        // The organization grant lost its only permission, so it no longer makes Enroute visible.
        assert_eq!(
            g.check::<AccountView>(&tree.enroute()),
            Err(Denied::AccountNotFound)
        );

        let mut nothing = grants(vec![builtin("account_admin", tree.at(tree.tower))]);
        nothing.intersect_key_scopes::<&str>(&[]);
        assert!(!nothing.is_visible(&tree.tower()));
    }
}
