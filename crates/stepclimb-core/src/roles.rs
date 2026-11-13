// SPDX-License-Identifier: AGPL-3.0-or-later
//! Roles: the eight built-in roles and their permission lists.
//!
//! The server reads a built-in role's permissions from [`BUILTIN`], never from its database row.
//! Organization admin is defined as [`perm::ALL`] and Instructor as Course designer's list plus
//! its own, so a permission added to the catalog or to Course designer reaches them without a
//! second edit.

use crate::perm::{self, Permission};

/// A role shipped with Stepclimb, the same in every organization.
#[derive(Debug)]
pub struct BuiltinRole {
    /// The stable key, stored in `roles.key`.
    pub key: &'static str,
    /// The display name, stored in `roles.name`.
    pub name: &'static str,
    permissions: RolePermissions,
}

#[derive(Debug)]
enum RolePermissions {
    /// Every entry in [`perm::ALL`].
    All,
    /// Exactly these keys.
    Keys(&'static [&'static str]),
    /// Another built-in role's permissions, then these keys.
    Extends {
        base: &'static BuiltinRole,
        own: &'static [&'static str],
    },
}

impl BuiltinRole {
    /// The role's permission keys: a base role's first, then its own, in definition order.
    pub fn permissions(&self) -> Vec<&'static str> {
        match self.permissions {
            RolePermissions::All => perm::ALL.iter().map(|p| p.key).collect(),
            RolePermissions::Keys(keys) => keys.to_vec(),
            RolePermissions::Extends { base, own } => {
                let mut keys = base.permissions();
                keys.extend_from_slice(own);
                keys
            }
        }
    }
}

static ORG_ADMIN: BuiltinRole = BuiltinRole {
    key: "org_admin",
    name: "Organization admin",
    permissions: RolePermissions::All,
};

/// Account admin manages an account subtree. It can view (not manage) the users in it, and view
/// and assign (not define) roles there; custom roles and user changes are organization-wide.
static ACCOUNT_ADMIN: BuiltinRole = BuiltinRole {
    key: "account_admin",
    name: "Account admin",
    permissions: RolePermissions::Keys(&[
        perm::AccountView::KEY,
        perm::AccountManage::KEY,
        perm::UserView::KEY,
        perm::RoleView::KEY,
        perm::RoleAssign::KEY,
    ]),
};

static COURSE_DESIGNER: BuiltinRole = BuiltinRole {
    key: "course_designer",
    name: "Course designer",
    permissions: RolePermissions::Keys(&[]),
};

static INSTRUCTOR: BuiltinRole = BuiltinRole {
    key: "instructor",
    name: "Instructor",
    permissions: RolePermissions::Extends {
        base: &COURSE_DESIGNER,
        own: &[],
    },
};

static GRADER: BuiltinRole = BuiltinRole {
    key: "grader",
    name: "Grader",
    permissions: RolePermissions::Keys(&[]),
};

static STUDENT: BuiltinRole = BuiltinRole {
    key: "student",
    name: "Student",
    permissions: RolePermissions::Keys(&[]),
};

static MANAGER: BuiltinRole = BuiltinRole {
    key: "manager",
    name: "Manager",
    permissions: RolePermissions::Keys(&[]),
};

static OBSERVER: BuiltinRole = BuiltinRole {
    key: "observer",
    name: "Observer",
    permissions: RolePermissions::Keys(&[]),
};

/// Every built-in role.
pub static BUILTIN: [&BuiltinRole; 8] = [
    &ORG_ADMIN,
    &ACCOUNT_ADMIN,
    &COURSE_DESIGNER,
    &INSTRUCTOR,
    &GRADER,
    &STUDENT,
    &MANAGER,
    &OBSERVER,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn role(key: &str) -> &'static BuiltinRole {
        BUILTIN
            .iter()
            .copied()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no built-in role {key}"))
    }

    fn set(keys: Vec<&'static str>) -> HashSet<&'static str> {
        keys.into_iter().collect()
    }

    #[test]
    fn the_eight_roles_with_unique_keys() {
        let keys: Vec<_> = BUILTIN.iter().map(|r| r.key).collect();
        assert_eq!(
            keys,
            [
                "org_admin",
                "account_admin",
                "course_designer",
                "instructor",
                "grader",
                "student",
                "manager",
                "observer",
            ]
        );
    }

    /// The custom role key format (spec 09 Q7), which built-in keys share.
    #[test]
    fn keys_match_the_role_key_format() {
        for r in BUILTIN {
            let mut chars = r.key.chars();
            let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
            let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            assert!(first_ok && rest_ok && r.key.len() <= 63, "{}", r.key);
            assert!(!r.name.trim().is_empty(), "{} has no name", r.key);
        }
    }

    #[test]
    fn org_admin_holds_every_catalog_key() {
        let all: Vec<_> = perm::ALL.iter().map(|p| p.key).collect();
        assert_eq!(role("org_admin").permissions(), all);
    }

    #[test]
    fn instructor_holds_every_course_designer_key() {
        let instructor = set(role("instructor").permissions());
        let designer = set(role("course_designer").permissions());
        assert!(instructor.is_superset(&designer));
    }

    #[test]
    fn extends_puts_the_base_first_then_its_own() {
        static BASE: BuiltinRole = BuiltinRole {
            key: "base",
            name: "Base",
            permissions: RolePermissions::Keys(&[perm::AccountView::KEY]),
        };
        let child = BuiltinRole {
            key: "child",
            name: "Child",
            permissions: RolePermissions::Extends {
                base: &BASE,
                own: &[perm::RoleView::KEY],
            },
        };
        assert_eq!(child.permissions(), ["account.view", "role.view"]);
    }

    #[test]
    fn account_admin_holds_exactly_its_list() {
        assert_eq!(
            set(role("account_admin").permissions()),
            set(vec![
                "account.view",
                "account.manage",
                "user.view",
                "role.view",
                "role.assign",
            ])
        );
    }

    #[test]
    fn roles_without_m0_permissions_hold_none() {
        for key in [
            "course_designer",
            "instructor",
            "grader",
            "student",
            "manager",
            "observer",
        ] {
            assert!(role(key).permissions().is_empty(), "{key}");
        }
    }

    #[test]
    fn every_role_key_is_in_the_catalog_once() {
        let catalog: HashSet<_> = perm::ALL.iter().map(|p| p.key).collect();
        for r in BUILTIN {
            let keys = r.permissions();
            assert_eq!(
                set(keys.clone()).len(),
                keys.len(),
                "{} repeats a key",
                r.key
            );
            for k in keys {
                assert!(catalog.contains(k), "{} lists {k}, not in perm::ALL", r.key);
            }
        }
    }
}
