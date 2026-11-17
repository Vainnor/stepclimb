// SPDX-License-Identifier: AGPL-3.0-or-later
//! The permission evaluator (docs/specs/m0/09-permissions-and-roles.md B3, B9, B16 to B18;
//! docs/specs/m1/02-courses-and-enrollments.md B16, B23 to B26).
//!
//! [`Grants::load`] reads a user's role assignments and active enrollments in one query and
//! turns them into grants: the permissions effective at the organization, at each account, and
//! at each course the user is enrolled in. A permission is effective at a grant's scope only
//! when the catalog lists it and its [`Permission::GRANTABLE`] includes that scope type (B3), so
//! an enrollment gives only permissions grantable at course scope; stored keys the catalog does
//! not know are left out and reported by [`Grants::ignored_keys`] (B9). An active enrollment is
//! the one course-scope grant (M1 spec 02 Q1): a course-scope role assignment grants nothing.
//!
//! Scopes nest. A grant at the organization contains every account and course; a grant at
//! account A contains A, every account whose path runs through A, and every course in those
//! accounts; an enrollment in course C contains C only. The checks take the target's path
//! ([`Target`]), so "does a grant contain the target" is "is the grant's account on the target's
//! path, or is the target the grant's course"; a course's path is its account's.
//!
//! A permission grantable only at course scope ([`perm::PermissionInfo::course_only`], such as
//! `course.participate`) can come only from an enrollment. The checks that ask a caller to hold
//! permissions at organization or account scope leave it out ([`Grants::lacks_any_of`],
//! [`Grants::lacks_any_held_by`]; M1 spec 02 B24a), since no caller could ever pass them
//! otherwise.
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
    /// A course, by its id and the path of the account it belongs to: the grants on the
    /// account's path and an enrollment in the course decide.
    Course {
        account_path: Vec<Uuid>,
        course_id: Uuid,
    },
}

impl Target {
    /// The account ids whose grants contain the target; empty for the organization.
    fn path(&self) -> &[Uuid] {
        match self {
            Self::Organization => &[],
            Self::Account(path) => path,
            Self::Course { account_path, .. } => account_path,
        }
    }

    /// The course whose enrollments contain the target, if the target is a course.
    fn course(&self) -> Option<Uuid> {
        match self {
            Self::Course { course_id, .. } => Some(*course_id),
            Self::Organization | Self::Account(_) => None,
        }
    }
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
    /// The course does not exist in the organization, or the principal holds nothing at a
    /// scope that contains it; answered alike, as for an account.
    #[error("the course does not exist")]
    CourseNotFound,
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
    /// Only courses where an active enrollment makes at least one permission effective.
    courses: BTreeMap<Uuid, BTreeSet<&'static str>>,
    ignored: BTreeSet<String>,
}

/// Where a grant takes effect, once read.
#[derive(Clone, Copy)]
enum Placement {
    Organization,
    Account(Uuid),
    Course(Uuid),
}

impl Placement {
    fn scope_type(self) -> ScopeType {
        match self {
            Self::Organization => ScopeType::Organization,
            Self::Account(_) => ScopeType::Account,
            Self::Course(_) => ScopeType::Course,
        }
    }
}

impl Grants {
    /// The grants of the user `user_id` in the scope's organization, from their role
    /// assignments and active enrollments, read in one query (B16; M1 spec 02 B23). A user the
    /// organization does not have holds nothing.
    pub async fn load(
        db: impl PgExecutor<'_>,
        scope: OrgScope,
        user_id: Uuid,
    ) -> Result<Self, role_assignments::Error> {
        let assignments = role_assignments::for_user_with_paths(db, scope, user_id).await?;
        Ok(Self::from_assignments(scope, assignments))
    }

    /// The grants a user's assignments and active enrollments in the scope's organization
    /// give, as [`role_assignments::for_user_with_paths`] reads them.
    #[must_use]
    pub fn from_assignments(
        scope: OrgScope,
        assignments: impl IntoIterator<Item = UserGrant>,
    ) -> Self {
        let mut grants = Self {
            scope,
            organization: BTreeSet::new(),
            accounts: BTreeMap::new(),
            courses: BTreeMap::new(),
            ignored: BTreeSet::new(),
        };
        for assignment in assignments {
            // `None` for a course-scope assignment, which grants nothing (M1 spec 02 Q1); its
            // unknown keys are still reported.
            let placement = match assignment.scope {
                GrantScope::Organization => Some(Placement::Organization),
                GrantScope::Account { id, .. } => Some(Placement::Account(id)),
                GrantScope::Enrolled(course_id) => Some(Placement::Course(course_id)),
                GrantScope::Course(_) => None,
            };
            let mut effective = BTreeSet::new();
            for key in assignment.permissions {
                let Some(p) = perm::find(&key) else {
                    grants.ignored.insert(key);
                    continue;
                };
                if let Some(placement) = placement
                    && p.grantable().contains(&placement.scope_type())
                {
                    effective.insert(p.key());
                }
            }
            if let Some(placement) = placement {
                grants.add(placement, effective);
            }
        }
        grants
    }

    fn add(&mut self, placement: Placement, effective: BTreeSet<&'static str>) {
        if effective.is_empty() {
            return;
        }
        match placement {
            Placement::Organization => self.organization.extend(effective),
            Placement::Account(id) => self.accounts.entry(id).or_default().extend(effective),
            Placement::Course(id) => self.courses.entry(id).or_default().extend(effective),
        }
    }

    /// Keep only the permissions an API key's scopes name: the key's rights are its scopes
    /// intersected with its owner's current grants (B16, spec 08 B27). A scope string that is
    /// not a catalog key matches nothing.
    pub fn intersect_key_scopes<S: AsRef<str>>(&mut self, key_scopes: &[S]) {
        let named = |key: &&str| key_scopes.iter().any(|s| s.as_ref() == *key);
        self.organization.retain(named);
        for keys in self.accounts.values_mut().chain(self.courses.values_mut()) {
            keys.retain(named);
        }
        self.accounts.retain(|_, keys| !keys.is_empty());
        self.courses.retain(|_, keys| !keys.is_empty());
    }

    /// The organization these grants are in.
    #[must_use]
    pub fn scope(&self) -> OrgScope {
        self.scope
    }

    /// Each organization or account scope where at least one permission is effective, with
    /// those permissions sorted: the organization first, then the accounts by id (spec 03 B22's
    /// `grants`). Permissions an account inherits from the organization or an ancestor are
    /// listed at that scope only. Enrollments are not listed: a learner may hold hundreds, and
    /// a course reports the caller's permissions there itself (M1 spec 02 B26).
    pub fn by_scope(&self) -> impl Iterator<Item = (ScopeId, Vec<&'static str>)> + '_ {
        let organization = (!self.organization.is_empty()).then(|| {
            (
                ScopeId::Organization(self.scope.org_id()),
                self.organization.iter().copied().collect(),
            )
        });
        let accounts = self
            .accounts
            .iter()
            .map(|(id, keys)| (ScopeId::Account(*id), keys.iter().copied().collect()));
        organization.into_iter().chain(accounts)
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
    /// visible to its members, and an account or a course when any permission is effective at
    /// a scope containing it, a course also through an enrollment in it (B17 step 4; M1 spec 02
    /// B24).
    #[must_use]
    pub fn is_visible(&self, target: &Target) -> bool {
        match target {
            Target::Organization => true,
            Target::Account(path) => {
                !self.organization.is_empty() || self.account_keys_on(path).next().is_some()
            }
            Target::Course {
                account_path,
                course_id,
            } => {
                !self.organization.is_empty()
                    || self.account_keys_on(account_path).next().is_some()
                    || self.courses.contains_key(course_id)
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
        if self
            .account_keys_on(target.path())
            .any(|keys| keys.contains(key))
        {
            return true;
        }
        target
            .course()
            .and_then(|id| self.courses.get(&id))
            .is_some_and(|keys| keys.contains(key))
    }

    /// Whether `keys` names a catalog permission, grantable at organization or account scope,
    /// that is not effective at a scope containing the target: the escalation rule of custom
    /// roles at organization scope (spec 09 B8, B9), of role assignments at their scope (B12
    /// step 5, B14) and of enrollments at their course (M1 spec 02 B17, B18). A key the catalog
    /// does not define grants nothing and does not count; nor does a permission grantable only
    /// at course scope, which no assignment makes effective, so asking a caller to hold it would
    /// refuse everyone (M1 spec 02 B24a, Q12).
    #[must_use]
    pub fn lacks_any_of<K: AsRef<str>>(&self, keys: &[K], target: &Target) -> bool {
        keys.iter().any(|key| {
            perm::find(key.as_ref())
                .is_some_and(|p| !p.course_only() && !self.holds_key(p.key(), target))
        })
    }

    /// Whether `other` has an effective permission, at any scope (its enrollments' courses
    /// included), that is not effective for these grants at organization scope: the escalation
    /// rule of the user routes (spec 03, owner decision of 2026-11-14), which an enrollment
    /// link's creator is held to at the link's use as well (spec 04 B6). A permission grantable
    /// only at course scope does not count (M1 spec 02 B24a).
    #[must_use]
    pub fn lacks_any_held_by(&self, other: &Grants) -> bool {
        other.all_keys().any(|key| {
            perm::find(key)
                .is_some_and(|p| !p.course_only() && !self.holds_key(key, &Target::Organization))
        })
    }

    /// Every effective permission, at every scope, with repeats.
    fn all_keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        let accounts = self.accounts.values().chain(self.courses.values());
        self.organization.iter().chain(accounts.flatten()).copied()
    }

    /// Whether `P` is effective at any scope, a course the principal is enrolled in included.
    #[must_use]
    pub fn holds_anywhere<P: Permission>(&self) -> bool {
        self.holds_key_anywhere(P::KEY)
    }

    /// Whether the permission with catalog key `key` is effective at any scope, a course the
    /// principal is enrolled in included.
    #[must_use]
    pub fn holds_key_anywhere(&self, key: &str) -> bool {
        self.all_keys().any(|held| held == key)
    }

    /// B17 steps 4 and 5 for a target that exists: not visible is
    /// [`Denied::AccountNotFound`] ([`Denied::CourseNotFound`] for a course), visible without
    /// `P` is [`Denied::Forbidden`].
    pub fn check<P: Permission>(&self, target: &Target) -> Result<(), Denied> {
        if !self.is_visible(target) {
            return Err(match target {
                Target::Course { .. } => Denied::CourseNotFound,
                Target::Organization | Target::Account(_) => Denied::AccountNotFound,
            });
        }
        if !self.holds::<P>(target) {
            return Err(Denied::Forbidden);
        }
        Ok(())
    }

    /// Every organization or account scope where `P` is effective, for a list filtered to what
    /// the principal may see (B18 `AnyScope`). Just the organization when `P` is effective
    /// there, since it contains every account; otherwise the accounts, sorted by id. The courses
    /// of the principal's enrollments are [`Self::courses_with`].
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

    /// Every course where an active enrollment makes `P` effective, sorted by id: the courses a
    /// list adds to [`Self::scopes_with`]'s scopes (M1 spec 02 B11).
    #[must_use]
    pub fn courses_with<P: Permission>(&self) -> Vec<Uuid> {
        self.courses
            .iter()
            .filter(|(_, keys)| keys.contains(P::KEY))
            .map(|(id, _)| *id)
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
        perm::{
            AccountManage, AccountView, CourseManage, CourseParticipate, CourseView,
            EnrollmentManage, OrgManage, RoleManage, UserView,
        },
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
            builtin("grader", tree.at(tree.tower)),
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
    fn course_scope_assignments_grant_nothing() {
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
    fn a_course_is_checked_through_its_accounts_path() {
        let tree = Tree::new();
        let in_tower = |course_id| Target::Course {
            account_path: vec![tree.root, tree.tower],
            course_id,
        };
        let in_enroute = Target::Course {
            account_path: vec![tree.root, tree.enroute],
            course_id: new_id(),
        };
        let g = grants(vec![assignment(&["course.manage"], tree.at(tree.tower))]);
        assert_eq!(g.check::<CourseManage>(&in_tower(new_id())), Ok(()));
        assert_eq!(
            g.check::<CourseView>(&in_tower(new_id())),
            Err(Denied::Forbidden)
        );
        assert_eq!(
            g.check::<CourseManage>(&in_enroute),
            Err(Denied::CourseNotFound)
        );
        let org = grants(vec![assignment(&["course.view"], GrantScope::Organization)]);
        assert_eq!(org.check::<CourseView>(&in_enroute), Ok(()));
        assert_eq!(
            org.check::<CourseManage>(&in_enroute),
            Err(Denied::Forbidden)
        );
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
    fn by_scope_lists_each_scope_with_its_sorted_permissions() {
        let tree = Tree::new();
        let g = grants(vec![
            assignment(&["role.view", "account.view"], tree.at(tree.tower)),
            assignment(&["account.manage"], tree.at(tree.tower)),
            assignment(&["user.view"], GrantScope::Organization),
            // Nothing effective at an account: not listed.
            assignment(&["organization.manage"], tree.at(tree.enroute)),
        ]);
        assert_eq!(
            g.by_scope().collect::<Vec<_>>(),
            [
                (ScopeId::Organization(g.scope().org_id()), vec!["user.view"]),
                (
                    ScopeId::Account(tree.tower),
                    vec!["account.manage", "account.view", "role.view"]
                ),
            ]
        );
        assert_eq!(grants(vec![]).by_scope().count(), 0);
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

    fn course_in(path: Vec<Uuid>, course_id: Uuid) -> Target {
        Target::Course {
            account_path: path,
            course_id,
        }
    }

    #[test]
    fn an_enrollment_grants_its_course_only_and_only_course_permissions() {
        let tree = Tree::new();
        let (enrolled, other) = (new_id(), new_id());
        let g = grants(vec![assignment(
            &[
                "course.view",
                "course.participate",
                "organization.manage",
                "account.view",
            ],
            GrantScope::Enrolled(enrolled),
        )]);
        let here = course_in(vec![tree.root, tree.tower], enrolled);
        let beside = course_in(vec![tree.root, tree.tower], other);
        assert_eq!(g.check::<CourseView>(&here), Ok(()));
        assert!(g.holds::<CourseParticipate>(&here));
        assert_eq!(g.check::<CourseView>(&beside), Err(Denied::CourseNotFound));
        assert_eq!(g.check::<CourseManage>(&here), Err(Denied::Forbidden));
        // Not grantable at course scope: no effect through an enrollment (B16, T14).
        assert!(!g.holds::<OrgManage>(&here));
        assert!(!g.holds_anywhere::<OrgManage>());
        assert!(!g.holds_anywhere::<AccountView>());
        assert!(!g.is_visible(&tree.tower()));
        assert!(g.holds_anywhere::<CourseView>());
        assert_eq!(g.courses_with::<CourseView>(), [enrolled]);
        assert!(g.courses_with::<CourseManage>().is_empty());
        assert_eq!(g.scopes_with::<CourseView>(), []);
        assert_eq!(g.by_scope().count(), 0, "/me lists no course grant (B26)");
    }

    #[test]
    fn an_enrollment_adds_to_the_grants_on_the_courses_path() {
        let tree = Tree::new();
        let course = new_id();
        let g = grants(vec![
            assignment(&["course.view"], tree.at(tree.tower)),
            assignment(&["enrollment.manage"], GrantScope::Enrolled(course)),
        ]);
        let target = course_in(vec![tree.root, tree.tower], course);
        assert!(g.holds::<CourseView>(&target));
        assert!(g.holds::<EnrollmentManage>(&target));
        assert!(!g.holds::<EnrollmentManage>(&tree.tower()));
    }

    #[test]
    fn an_api_key_narrows_enrollment_grants_too() {
        let course = new_id();
        let mut g = grants(vec![assignment(
            &["course.view", "course.participate"],
            GrantScope::Enrolled(course),
        )]);
        g.intersect_key_scopes(&["course.view"]);
        let target = course_in(vec![new_id()], course);
        assert!(g.holds::<CourseView>(&target));
        assert!(!g.holds::<CourseParticipate>(&target));
        g.intersect_key_scopes::<&str>(&[]);
        assert!(!g.is_visible(&target));
        assert!(g.courses_with::<CourseView>().is_empty());
    }

    #[test]
    fn lacks_any_of_leaves_out_course_only_and_unknown_keys() {
        let tree = Tree::new();
        let admin = grants(vec![builtin("org_admin", GrantScope::Organization)]);
        let every: Vec<&str> = perm::ALL.iter().map(|p| p.key()).collect();
        // The Organization admin holds every key that can be held at organization scope.
        assert!(!admin.lacks_any_of(&every, &Target::Organization));
        assert!(!admin.lacks_any_of(&["course.participate", "gone.key"], &tree.tower()));
        let account = grants(vec![builtin("account_admin", tree.at(tree.tower))]);
        let course = course_in(vec![tree.root, tree.tower], new_id());
        assert!(!account.lacks_any_of(&["course.view", "course.participate"], &course));
        // A course-only key does not excuse the others (T12a).
        assert!(account.lacks_any_of(&["course.participate", "organization.manage"], &course));
        assert!(account.lacks_any_of(&["course.view"], &tree.enroute()));
    }

    #[test]
    fn lacks_any_held_by_counts_enrollments_but_not_course_only_keys() {
        let course = new_id();
        let caller = grants(vec![assignment(&["user.manage"], GrantScope::Organization)]);
        let student = grants(vec![builtin("student", GrantScope::Enrolled(course))]);
        let participant = grants(vec![assignment(
            &["course.participate"],
            GrantScope::Enrolled(course),
        )]);
        let instructor = grants(vec![builtin("instructor", GrantScope::Enrolled(course))]);
        assert!(!caller.lacks_any_held_by(&participant));
        assert!(
            caller.lacks_any_held_by(&student),
            "course.view at a course counts"
        );
        assert!(caller.lacks_any_held_by(&instructor));
        let admin = grants(vec![builtin("org_admin", GrantScope::Organization)]);
        assert!(!admin.lacks_any_held_by(&instructor));
        assert!(!admin.lacks_any_held_by(&student));
        assert!(!admin.lacks_any_held_by(&admin));
    }

    #[test]
    fn holds_key_anywhere_includes_enrollments() {
        let g = grants(vec![assignment(
            &["course.participate"],
            GrantScope::Enrolled(new_id()),
        )]);
        assert!(g.holds_key_anywhere("course.participate"));
        assert!(!g.holds_key_anywhere("course.view"));
    }
}
