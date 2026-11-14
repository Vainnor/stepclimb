// SPDX-License-Identifier: Apache-2.0
// Generated from openapi/stepclimb.json by scripts/generate.mjs. Do not edit; run
// `just openapi` instead.
export interface paths {
    "/api/v1/accounts": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * List an account's children, or without `parent_id` the topmost accounts the caller can see:
         *     the root for a holder of `account.view` at organization scope, otherwise each account where
         *     the caller holds it below no other such account. Sorted by name, then id.
         */
        get: operations["listAccounts"];
        put?: never;
        /** Create an account under `parent_id`. */
        post: operations["createAccount"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/accounts/{account_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one account. */
        get: operations["getAccount"];
        put?: never;
        post?: never;
        /** Delete an account that has no child accounts, and the role assignments scoped to it. */
        delete: operations["deleteAccount"];
        options?: never;
        head?: never;
        /** Rename an account. A patch without `name` changes nothing. */
        patch: operations["updateAccount"];
        trace?: never;
    };
    "/api/v1/accounts/{account_id}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Move an account and its subtree under `parent_id`. */
        post: operations["moveAccount"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/permissions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the permission catalog. */
        get: operations["listPermissions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/role-assignments": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the role assignments at scopes where the caller holds `role.view`, by id. */
        get: operations["listRoleAssignments"];
        put?: never;
        /** Grant a role to a user at a scope. */
        post: operations["createRoleAssignment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/role-assignments/{assignment_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove a role assignment. */
        delete: operations["deleteRoleAssignment"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/roles": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the roles: the built-in roles, then the organization's custom roles, each by key. */
        get: operations["listRoles"];
        put?: never;
        /** Create a custom role. */
        post: operations["createRole"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/roles/{role_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one role. */
        get: operations["getRole"];
        put?: never;
        post?: never;
        /** Delete a custom role that has no assignments. */
        delete: operations["deleteRole"];
        options?: never;
        head?: never;
        /** Change a custom role's name or permissions; holders see the change on their next request. */
        patch: operations["updateRole"];
        trace?: never;
    };
    "/api/v1/me": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the signed-in user. */
        get: operations["getMe"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/organization": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the caller's organization. */
        get: operations["getOrganization"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /** Rename the caller's organization. A patch without `name` changes nothing. */
        patch: operations["updateOrganization"];
        trace?: never;
    };
    "/api/v1/organization/entitlements": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Read the organization's plan, every catalog feature with whether the plan includes it, and
         *     every catalog limit with its value.
         */
        get: operations["getOrganizationEntitlements"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/organization/settings": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization settings, every key with its effective value. */
        get: operations["getOrganizationSettings"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Change the settings the patch names; the others keep their value. A patch naming no setting
         *     writes nothing.
         */
        patch: operations["updateOrganizationSettings"];
        trace?: never;
    };
    "/api/v1/users": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the organization's users, sorted by display name, then id. */
        get: operations["listUsers"];
        put?: never;
        /** Create a user, `local` and `active`, and link the identities given to them. */
        post: operations["createUser"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/users/{user_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read one user. */
        get: operations["getUser"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * Change a user's names, email, locale, time zone or status. A patch naming nothing changes
         *     nothing.
         */
        patch: operations["updateUser"];
        trace?: never;
    };
    "/auth/sign-out": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * End the browser's session and clear its cookie.
         * @description Answers 204 whether or not the cookie named a session. It needs no permission and no valid
         *     session, so a browser holding an expired or unknown cookie can still clear it.
         */
        post: operations["signOut"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/healthz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The server process is running. */
        get: operations["healthz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/readyz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The server can take traffic: the database answered within two seconds. */
        get: operations["readyz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** @description An account of the caller's organization. */
        Account: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /**
             * Format: int32
             * @description 0 for the root, 1 for its children, and so on; at most 16.
             */
            depth: number;
            has_children: boolean;
            /** Format: uuid */
            id: string;
            name: string;
            /**
             * Format: uuid
             * @description `null` for the root only. Shown even when the parent is not visible to the caller.
             */
            parent_id: string | null;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The new parent of a moved account. */
        AccountMove: {
            /**
             * Format: uuid
             * @description The new parent; you must hold `account.manage` at a scope containing it. The account's
             *     current parent changes nothing.
             */
            parent_id: string;
        };
        /**
         * @description A change to an account; an omitted member is left as it is. A member sent as `null` is
         *     malformed: omit it instead.
         */
        AccountPatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string;
        };
        /**
         * @description Who owns an assignment: an admin of this organization (`local`), or the identity provider,
         *     sync API, SCIM or connector that wrote it, in which case it cannot be removed here.
         * @enum {string}
         */
        AssignmentSource: "local" | "idp" | "sync" | "scim" | "connector";
        /** @description The organization's plan and what it allows: every feature and limit of the server's catalogs. */
        Entitlements: {
            /** @description Every catalog feature, by key, with whether the plan includes it. */
            features: {
                [key: string]: boolean;
            };
            /** @description Every catalog limit, by key, with the most units the plan allows, or `null` for no limit. */
            limits: {
                [key: string]: number | null;
            };
            plan: components["schemas"]["PlanRef"];
        };
        /** @description One rule a request body broke, in a 422's `errors`. */
        FieldError: {
            /** @description The stable code of the rule, such as `settings.unknown_key`. */
            code: string;
            /** @description A JSON Pointer (RFC 6901) to the member that broke the rule, such as `/name`. */
            field: string;
        };
        /**
         * @description A scope a grant or a role assignment applies at: the organization, an account or a course,
         *     by id. An organization scope's `id` is the organization's id.
         */
        GrantScope: {
            /** Format: uuid */
            id: string;
            type: components["schemas"]["ScopeType"];
        };
        /** @description A probe's answer. */
        Health: {
            status: components["schemas"]["HealthStatus"];
        };
        /**
         * @description `ok` when the probe passed, `unavailable` when it did not.
         * @enum {string}
         */
        HealthStatus: "ok" | "unavailable";
        /** @description What `GET /api/v1/me` answers. */
        Me: {
            /** @description The keys of the catalog features the organization's plan includes, sorted. */
            features: string[];
            /**
             * @description Each scope where the caller holds at least one effective permission, with those
             *     permissions sorted: the organization first, then accounts by id. A permission held at a
             *     scope also takes effect in every account inside it, which are not listed again.
             */
            grants: components["schemas"]["MeGrant"][];
            organization: components["schemas"]["Organization"];
            user: components["schemas"]["MeUser"];
        };
        /** @description The permissions the caller holds at one scope. */
        MeGrant: {
            /** @description Catalog keys, sorted. */
            permissions: string[];
            scope: components["schemas"]["GrantScope"];
        };
        /** @description The signed-in user. */
        MeUser: {
            display_name: string;
            email: string | null;
            /** @description `""` when the identity provider sent none. */
            family_name: string;
            /** @description `""` when the identity provider sent none. */
            given_name: string;
            /** Format: uuid */
            id: string;
            /** @description `en` in M0. */
            locale: string;
            source: components["schemas"]["UserSource"];
            status: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz: string;
        };
        /** @description An account to create. */
        NewAccount: {
            /** @description Trimmed; 1 to 200 characters. */
            name: string;
            /**
             * Format: uuid
             * @description The parent account; you must hold `account.manage` at a scope containing it.
             */
            parent_id: string;
        };
        /** @description A custom role to create. */
        NewRole: {
            /**
             * @description A lowercase letter, then up to 62 lowercase letters, digits or underscores; not the key
             *     of a built-in role or another custom role of the organization.
             */
            key: string;
            /** @description Trimmed; 1 to 200 characters. */
            name: string;
            /**
             * @description Catalog keys (`GET /api/v1/admin/permissions`), each at most once and each held by the
             *     caller at organization scope.
             */
            permissions: string[];
        };
        /** @description An assignment to create. */
        NewRoleAssignment: {
            /**
             * Format: uuid
             * @description A built-in role other than `manager` and `observer`, or a custom role of the
             *     organization.
             */
            role_id: string;
            /** @description The organization (its own id) or an account of it. `course` is not available yet. */
            scope: components["schemas"]["ScopeRef"];
            /** Format: uuid */
            user_id: string;
        };
        /** @description A user to create. The new user is `local` and `active`, and has no local credential. */
        NewUser: {
            /** @description Trimmed; 1 to 200 characters. */
            display_name: string;
            /** @description Trimmed; at most 254 characters; unique in the organization in any letter case. */
            email?: string | null;
            /** @description At most 200 characters; unique in the organization. */
            external_id?: string | null;
            /** @description At most 200 characters; `""` when omitted. */
            family_name?: string;
            /** @description At most 200 characters; `""` when omitted. */
            given_name?: string;
            /**
             * @description Identities to link to the new user, so they can sign in with them the first time; at
             *     most 20.
             */
            identities?: components["schemas"]["NewUserIdentity"][];
            /** @description `en`, the default and the only locale in M0. */
            locale?: string;
            /** @description An IANA time zone; the organization's `default_tz` when omitted. */
            tz?: string;
        };
        /** @description An identity to link to a new user: the provider's subject at the provider with this key. */
        NewUserIdentity: {
            provider_key: string;
            /** @description Stored exactly as given; not empty. */
            subject: string;
        };
        /**
         * @description Every organization setting with its effective value: the stored one, or the setting's
         *     default when none is stored.
         */
        OrgSettings: {
            /** @description The IANA time zone a user created without one gets; `UTC` unless set. */
            default_tz: string;
        };
        /**
         * @description A partial change to the organization settings: one optional member per setting; only the
         *     members present change. A member that is no setting is refused.
         */
        OrgSettingsPatch: {
            /** @description An IANA time zone, matched in any letter case and stored in the database's spelling. */
            default_tz?: string;
        };
        /** @description The caller's organization. */
        Organization: {
            /** Format: uuid */
            id: string;
            name: string;
            /** @description Fixed when the organization is created. */
            slug: string;
        };
        /**
         * @description A change to the organization; an omitted member is left as it is. A member sent as `null`
         *     is malformed: omit it instead.
         */
        OrganizationPatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string;
        };
        /** @description One page of a list. */
        Page_Account: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /**
                 * Format: int32
                 * @description 0 for the root, 1 for its children, and so on; at most 16.
                 */
                depth: number;
                has_children: boolean;
                /** Format: uuid */
                id: string;
                name: string;
                /**
                 * Format: uuid
                 * @description `null` for the root only. Shown even when the parent is not visible to the caller.
                 */
                parent_id: string | null;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_Role: {
            /** @description The items, in the list's sort order. */
            items: {
                /** @description Built-in roles are the same in every organization and cannot be changed or deleted. */
                builtin: boolean;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** Format: uuid */
                id: string;
                key: string;
                name: string;
                /** @description The permission keys the role grants. A built-in role's are this server's list for it. */
                permissions: string[];
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_RoleAssignment: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                /** @description The owning system's id for the assignment; `null` for a `local` one. */
                external_id: string | null;
                /** Format: uuid */
                id: string;
                /** Format: uuid */
                role_id: string;
                scope: components["schemas"]["GrantScope"];
                source: components["schemas"]["AssignmentSource"];
                /** Format: uuid */
                user_id: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One page of a list. */
        Page_User: {
            /** @description The items, in the list's sort order. */
            items: {
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                created_at: string;
                display_name: string;
                email: string | null;
                /** @description The owning system's id for the user; `null` when there is none. */
                external_id: string | null;
                /** @description `""` when none was given. */
                family_name: string;
                /** @description `""` when none was given. */
                given_name: string;
                /** Format: uuid */
                id: string;
                /** @description `en` in M0. */
                locale: string;
                source: components["schemas"]["UserSource"];
                status: components["schemas"]["UserStatus"];
                /** @description An IANA time zone. */
                tz: string;
                /**
                 * Format: date-time
                 * @description RFC 3339, UTC.
                 */
                updated_at: string;
            }[];
            /** @description Pass this as `cursor` to get the next page; `null` on the last page. */
            next_cursor: string | null;
        };
        /** @description One permission of the catalog. */
        PermissionInfo: {
            /** @description The scope types at which a role assignment grants it. */
            grantable: components["schemas"]["ScopeType"][];
            /** @description The key roles list, `area.action`. */
            key: string;
        };
        /** @description Which plan the organization runs on and how it got it. */
        PlanRef: {
            /** @description The plan's key; `unlimited` for a self-hosted organization. */
            key: string;
            source: components["schemas"]["PlanSource"];
        };
        /**
         * @description How the organization got its plan: a subscription (`stripe`), granted by the operator
         *     (`grant`), the non-profit plan (`nonprofit`), or the self-hosted `unlimited` plan
         *     (`selfhost`).
         * @enum {string}
         */
        PlanSource: "stripe" | "grant" | "nonprofit" | "selfhost";
        /** @description The body of every error response (RFC 9457). */
        Problem: {
            /** @description The stable error code, `area.reason`. */
            code: string;
            /** @description A sentence for a human reader. Clients branch on `code`, never on this text. */
            detail: string;
            /** @description On a 422, each rule the body broke. */
            errors?: components["schemas"]["FieldError"][];
            /**
             * @description On `entitlement.limit_reached`, the key of the limit the request would pass, such as
             *     `users`.
             */
            limit?: string | null;
            /**
             * Format: int64
             * @description On `entitlement.limit_reached`, the plan's value for that limit.
             */
            max?: number | null;
            /**
             * Format: int32
             * @description The HTTP status code, repeated.
             */
            status: number;
            /** @description The HTTP status phrase. */
            title: string;
            /** @description `about:blank` until the docs site documents each code. */
            type: string;
        };
        /** @description A built-in role or a custom role of the organization. */
        Role: {
            /** @description Built-in roles are the same in every organization and cannot be changed or deleted. */
            builtin: boolean;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            key: string;
            name: string;
            /** @description The permission keys the role grants. A built-in role's are this server's list for it. */
            permissions: string[];
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description A role granted to a user at a scope. */
        RoleAssignment: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** @description The owning system's id for the assignment; `null` for a `local` one. */
            external_id: string | null;
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            role_id: string;
            scope: components["schemas"]["GrantScope"];
            source: components["schemas"]["AssignmentSource"];
            /** Format: uuid */
            user_id: string;
        };
        /** @description A change to a custom role; an omitted member is left as it is. */
        RolePatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string | null;
            /**
             * @description The role's whole new list, each catalog key at most once. The caller must hold at
             *     organization scope every key it adds and every key it drops.
             */
            permissions?: string[] | null;
        };
        /** @description The scope a new assignment takes effect at. */
        ScopeRef: {
            /**
             * Format: uuid
             * @description The organization's own id for `organization`.
             */
            id: string;
            type: components["schemas"]["ScopeType"];
        };
        /**
         * @description The kind of scope a permission can take effect at.
         * @enum {string}
         */
        ScopeType: "organization" | "account" | "course";
        /** @description A user of the caller's organization. */
        User: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            display_name: string;
            email: string | null;
            /** @description The owning system's id for the user; `null` when there is none. */
            external_id: string | null;
            /** @description `""` when none was given. */
            family_name: string;
            /** @description `""` when none was given. */
            given_name: string;
            /** Format: uuid */
            id: string;
            /** @description `en` in M0. */
            locale: string;
            source: components["schemas"]["UserSource"];
            status: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /**
         * @description A change to a user; an omitted member is left as it is. A member sent as `null` is
         *     malformed, except `email`, where `null` removes the email.
         */
        UserPatch: {
            /** @description Trimmed; 1 to 200 characters. Refused for a user an outside source owns. */
            display_name?: string;
            /**
             * @description Trimmed; at most 254 characters; `null` removes it. Refused for a user an outside source
             *     owns.
             */
            email?: string | null;
            /** @description At most 200 characters. Refused for a user an outside source owns. */
            family_name?: string;
            /** @description At most 200 characters. Refused for a user an outside source owns. */
            given_name?: string;
            /** @description `en` in M0. */
            locale?: string;
            /**
             * @description `suspended` ends every session of the user on its next request; you cannot suspend
             *     yourself.
             */
            status?: components["schemas"]["UserStatus"];
            /** @description An IANA time zone. */
            tz?: string;
        };
        /**
         * @description Where the user record came from: made by an admin (`local`), at sign-in through an identity
         *     provider (`idp`), or by the sync API, SCIM or a connector, which own it.
         * @enum {string}
         */
        UserSource: "local" | "idp" | "sync" | "scim" | "connector";
        /**
         * @description Whether a user may sign in.
         * @enum {string}
         */
        UserStatus: "active" | "suspended";
    };
    responses: {
        /** @description `account.not_found`: no account with this id exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        AccountNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role_assignment.exists`: the user already holds the role at the scope. `role.unknown_builtin`: the role is a built-in role of another Stepclimb release. */
        AssignmentCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `role.assign` at the scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site. */
        AssignmentCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `role.not_found`: no built-in role and no custom role of the organization has this id. `account.not_found`: the scope names no account of the organization, or one you cannot see. `org.not_found`: the scope names another organization, or the request's organization cannot be determined. */
        AssignmentCreateNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.unknown_builtin`: the assignment's role is a built-in role of another Stepclimb release. `role_assignment.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the assignment. `role_assignment.last_org_admin`: it is the organization's last organization-scope Organization admin assignment held by an active user. */
        AssignmentDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `role.assign` at the assignment's scope. `permission.escalation`: the role has a catalog permission the caller does not hold at the scope, counting permissions that take no effect there. `auth.csrf_refused`: the request did not come from this site. */
        AssignmentDeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role_assignment.not_found`: no assignment of the organization has this id, or its scope is hidden from you. `org.not_found`: the request's organization cannot be determined. */
        AssignmentDeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the scope filter names no account of the organization, or one you cannot see. `org.not_found`: the scope filter names another organization, or the request's organization cannot be determined. */
        AssignmentListNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body, a path parameter or a query parameter does not parse or does not match the schema, or `limit` is outside 1 to 200. `pagination.invalid_cursor`: the cursor does not decode. */
        BadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.key_taken`: a built-in role or another custom role of the organization has the key. */
        CreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you hold `account.manage` nowhere, or you can see the parent without holding it there. `auth.csrf_refused`: the request did not come from this site. */
        CreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.csrf_refused`: an unsafe request without a bearer credential did not come from this site: `Sec-Fetch-Site` is not `same-origin`, `Origin` is sent but is not an allowed origin, or neither header is sent. */
        CsrfRefused: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.builtin_immutable`: built-in roles cannot be deleted. `role.in_use`: the role still has assignments; remove them first. */
        DeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include custom roles. `permission.denied`: the caller does not hold `role.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        DeleteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the account does not exist or you cannot see its parent. `org.not_found`: the request's organization cannot be determined. */
        DeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `internal.error`: the server failed; the cause is in its log. */
        InternalError: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the account but do not hold `account.manage` at a scope containing it (for a delete, the account's parent). `auth.csrf_refused`: the request did not come from this site. */
        ManageForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.method_not_allowed`: the path does not take this method; `Allow` lists the methods it takes. */
        MethodNotAllowed: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.root_immutable`: the root account cannot be moved. `account.move_into_descendant`: the destination is the account itself or one of its descendants. `account.too_deep`: an account of the moved subtree would end more than 16 levels below the root. */
        MoveConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: you can see the account's parent but do not hold `account.manage` there. `permission.escalation`: the destination would give you, over the moved account, a permission you do not hold over it now. `auth.csrf_refused`: the request did not come from this site. */
        MoveForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: the account does not exist or you cannot see its parent, or the destination `parent_id` does not exist or is outside every subtree where you hold `account.manage`. `org.not_found`: the request's organization cannot be determined. */
        MoveNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `org.none`: the instance has no organization yet. */
        NoOrganization: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `organization.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        OrgWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `org.not_found`: the request's organization cannot be determined. */
        OrganizationNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `account.not_found`: no account with `parent_id` exists in the organization, or you cannot see it. `org.not_found`: the request's organization cannot be determined. */
        ParentNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.too_large`: the body is larger than the server accepts. */
        PayloadTooLarge: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold the operation's permission at a scope containing its target. */
        PermissionDenied: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.rate_limited`: too many sign-in or token requests from this client address, or too many failed password or TOTP checks for this account; `Retry-After` gives the seconds until the limit's window closes. */
        RateLimited: {
            headers: {
                /** @description Seconds to wait before trying again, at least 1. */
                "Retry-After"?: number;
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.not_found`: no built-in role and no custom role of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        RoleNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.not_found`: no API operation has this path. */
        RouteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the patch breaks a rule; `errors` lists each one. `settings.unknown_key` marks a key that is no organization setting; `tz.unknown` a `default_tz` that is no IANA time zone. */
        SettingsInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.unauthenticated`: no credential, or one that names no session of the organization. `auth.session_expired`: the session passed its idle or absolute expiry. `auth.suspended`: the user is suspended. */
        Unauthenticated: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the body parses but breaks a rule; `errors` lists each one. */
        UnprocessableEntity: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.unsupported_media_type`: the body is not JSON. */
        UnsupportedMediaType: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `role.builtin_immutable`: built-in roles cannot be changed. */
        UpdateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.external_id_taken`: another user of the organization has this external id. `user.email_taken`: another user of the organization has this email, in any letter case. `identity.taken`: one of `identities` is already linked to a user. */
        UserCreateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `entitlement.limit_reached`: the organization's plan allows no more users; `limit` and `max` name the limit and its value. `auth.csrf_refused`: the request did not come from this site. */
        UserCreateForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `org.not_found`: the request's organization cannot be determined. */
        UserNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.externally_managed`: an identity provider, the sync API, SCIM or a connector owns the user's names and email; change them there. `user.cannot_suspend_self`: you cannot suspend yourself. `user.email_taken`: another user of the organization has this email, in any letter case. `role_assignment.last_org_admin`: the user is the organization's last active Organization admin. */
        UserUpdateConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the patch changes `email` or `status` of a user who holds a permission the caller does not hold at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        UserWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `entitlement.feature_unavailable`: the organization's plan does not include custom roles. `permission.denied`: the caller does not hold `role.manage` at organization scope. `permission.escalation`: the caller does not hold, at organization scope, a permission the role gains or loses. `auth.csrf_refused`: the request did not come from this site. */
        WriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
    };
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    listAccounts: {
        parameters: {
            query?: {
                /**
                 * @description List this account's children. Omitted, list the topmost accounts you can see with
                 *     `account.view`.
                 */
                parent_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of accounts */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["ParentNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createAccount: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewAccount"];
            };
        };
        responses: {
            /** @description The account is created */
            201: {
                headers: {
                    /** @description The account's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CreateForbidden"];
            404: components["responses"]["ParentNotFound"];
            409: components["responses"]["CreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The account */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["AccountNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The account is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ManageForbidden"];
            404: components["responses"]["DeleteNotFound"];
            409: components["responses"]["DeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AccountPatch"];
            };
        };
        responses: {
            /** @description The account after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["ManageForbidden"];
            404: components["responses"]["AccountNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    moveAccount: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The account's id. */
                account_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["AccountMove"];
            };
        };
        responses: {
            /** @description The account after the move */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Account"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MoveForbidden"];
            404: components["responses"]["MoveNotFound"];
            409: components["responses"]["MoveConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listPermissions: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Every permission, in catalog order */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PermissionInfo"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listRoleAssignments: {
        parameters: {
            query?: {
                /** @description Only this user's assignments. */
                user_id?: string;
                /** @description Only assignments at this exact scope; needs `scope_id`. */
                scope_type?: components["schemas"]["ScopeType"];
                /** @description The scope's id (the organization's own id for `organization`); needs `scope_type`. */
                scope_id?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of assignments */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_RoleAssignment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["AssignmentListNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createRoleAssignment: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewRoleAssignment"];
            };
        };
        responses: {
            /** @description The assignment is created */
            201: {
                headers: {
                    /** @description The assignment's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RoleAssignment"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AssignmentCreateForbidden"];
            404: components["responses"]["AssignmentCreateNotFound"];
            409: components["responses"]["AssignmentCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteRoleAssignment: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The assignment's id. */
                assignment_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The assignment is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["AssignmentDeleteForbidden"];
            404: components["responses"]["AssignmentDeleteNotFound"];
            409: components["responses"]["AssignmentDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listRoles: {
        parameters: {
            query?: {
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of roles */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createRole: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewRole"];
            };
        };
        responses: {
            /** @description The role is created */
            201: {
                headers: {
                    /** @description The role's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["WriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["CreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The role */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["RoleNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The role is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["DeleteForbidden"];
            404: components["responses"]["RoleNotFound"];
            409: components["responses"]["DeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateRole: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The role's id. */
                role_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RolePatch"];
            };
        };
        responses: {
            /** @description The changed role */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Role"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["WriteForbidden"];
            404: components["responses"]["RoleNotFound"];
            409: components["responses"]["UpdateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getMe: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The signed-in user and their organization */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Me"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganization: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The caller's organization */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Organization"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateOrganization: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["OrganizationPatch"];
            };
        };
        responses: {
            /** @description The organization after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Organization"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["OrgWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganizationEntitlements: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The plan and what it allows */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Entitlements"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getOrganizationSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Every setting with its effective value */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OrgSettings"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateOrganizationSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["OrgSettingsPatch"];
            };
        };
        responses: {
            /** @description Every setting after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OrgSettings"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["OrgWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["SettingsInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listUsers: {
        parameters: {
            query?: {
                status?: components["schemas"]["UserStatus"];
                source?: components["schemas"]["UserSource"];
                /** @description A prefix of the display name or the email, in any letter case. */
                q?: string;
                /** @description The `next_cursor` of the previous page; omit it for the first page. */
                cursor?: string;
                /** @description How many items to return, 1 to 200; 50 when omitted. */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description One page of users */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Page_User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createUser: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewUser"];
            };
        };
        responses: {
            /** @description The user is created */
            201: {
                headers: {
                    /** @description The user's path */
                    Location?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UserCreateForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["UserCreateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getUser: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The user */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["UserNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateUser: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UserPatch"];
            };
        };
        responses: {
            /** @description The user after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["User"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["UserWriteForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["UserUpdateConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    signOut: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The session, if there was one, is ended and the cookie cleared */
            204: {
                headers: {
                    /** @description Clears `__Host-sc_session` (`Max-Age=0`) */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    healthz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The server is running */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
        };
    };
    readyz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The database answered */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
            /** @description The database did not answer in time */
            503: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Health"];
                };
            };
        };
    };
}
