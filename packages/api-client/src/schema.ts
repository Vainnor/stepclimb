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
    "/api/v1/admin/branding": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's branding. */
        get: operations["getBranding"];
        /**
         * Replace the organization's branding. The server generates the light, dark and
         *     high-contrast themes from the inputs and saves them only when every contrast pair passes;
         *     `/theme.css` serves the new stylesheet from then on.
         */
        put: operations["updateBranding"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/local-sign-in": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** Read the organization's local sign-in switches. */
        get: operations["getLocalSignIn"];
        /** Set every local sign-in switch. A body equal to the current switches changes nothing. */
        put: operations["updateLocalSignIn"];
        post?: never;
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
    "/api/v1/admin/users/{user_id}/enrollment-links": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Create an enrollment link for a user, replacing their earlier unused one. */
        post: operations["createEnrollmentLink"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
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
    "/api/v1/me/appearance": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        /**
         * Set the signed-in user's theme and density. The answer also sets the `__Host-sc_appearance`
         *     cookie, so this browser's signed-out screens use the same choice.
         */
        put: operations["updateMyAppearance"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/identities": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List the signed-in user's identities, oldest first, without the attributes providers sent. */
        get: operations["listMyIdentities"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/identities/{identity_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove one of the signed-in user's identities. You must keep a way to sign in. */
        delete: operations["deleteMyIdentity"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List your passkeys, oldest first. */
        get: operations["listMyPasskeys"];
        put?: never;
        /** Add the passkey whose registration `POST /api/v1/me/passkeys/start` began in this browser. */
        post: operations["createMyPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Start adding a passkey to your account. */
        post: operations["startMyPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/passkeys/{passkey_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Delete one of your passkeys. You must keep a way to sign in. */
        delete: operations["deleteMyPasskey"];
        options?: never;
        head?: never;
        /** Rename one of your passkeys. */
        patch: operations["updateMyPasskey"];
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
    "/api/v1/users/{user_id}/identities": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** List a user's identities, oldest first. */
        get: operations["listUserIdentities"];
        put?: never;
        /** Link an identity to a user, so they can sign in with it. */
        post: operations["createUserIdentity"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/users/{user_id}/identities/{identity_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** Remove an identity from a user. The user must keep a way to sign in. */
        delete: operations["deleteUserIdentity"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/enrollment/passkey": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Register a passkey through an enrollment link and sign in with it. */
        post: operations["enrollPasskey"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/enrollment/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Open an enrollment link: say whose it is and what it can set up, without using it. */
        post: operations["startEnrollment"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/passkey/finish": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Finish signing in with a passkey. */
        post: operations["finishPasskeySignIn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/local/passkey/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** Start signing in with a passkey. */
        post: operations["startPasskeySignIn"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
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
    "/auth/{provider}/callback": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Finish a sign-in with an outside identity provider.
         * @description A browser navigation: the provider's redirect back after sign-in, with `code` and `state` or with `error` and `state`. It needs the `__Host-sc_flow` cookie of the flow the start route began, and clears it. Every refusal is a 303 to `/sign-in?error=<code>`: `auth.provider_not_found` (no enabled provider with this key), `auth.flow_invalid` (no flow cookie, a used or unknown flow, a flow for another provider, or another `state`), `auth.flow_expired` (the flow is older than 10 minutes), `auth.provider_denied` (the person declined at the provider), `auth.provider_error` (the provider reported an error, or its token, key set or userinfo answer could not be used), `auth.id_token_invalid` (the ID token failed a check), `auth.subject_missing` (the ID token's `sub` or the OAuth 2 userinfo subject is missing, empty, holds NUL or is longer than 255 characters), the admission refusals `auth.suspended`, `auth.signup_closed` and `entitlement.limit_reached` (OAuth 2), `auth.provider_unavailable` (an OIDC sign-in verified, which this release does not complete yet) or `auth.rate_limited` (with `Retry-After`). An admitted OAuth 2 sign-in sets `__Host-sc_session` and is a 303 to the flow's `return_to`.
         */
        get: operations["finishProviderSignIn"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/auth/{provider}/start": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * Start a sign-in with an outside identity provider.
         * @description A browser navigation. Sends the browser to the provider's authorization endpoint with `state` and a PKCE S256 challenge (for an OIDC provider also a `nonce`; for an OAuth 2 provider whose `pkce` is off, no challenge), bound to the sign-in flow whose `__Host-sc_flow` cookie this answer sets. Every refusal is a 303 to `/sign-in?error=<code>`: `auth.provider_not_found` (no enabled provider with this key), `auth.provider_unavailable` (the provider's discovery document could not be read or names another issuer, its stored config does not read, or its authorization URL is refused by the outbound URL policy) or `auth.rate_limited` (with `Retry-After`).
         */
        get: operations["startProviderSignIn"];
        put?: never;
        post?: never;
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
    "/theme.css": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** The stylesheet of the request's organization: its saved branding's, or the default brand's. */
        get: operations["getThemeCss"];
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
        /** @description The admission switches as the organization settings show them. */
        AdmissionSwitches: {
            /**
             * @description Whether a person the organization has no user for is created as a user when they first
             *     sign in through an identity provider; `false` unless set. Off, they are refused with
             *     `auth.signup_closed`.
             */
            signup_allowed: boolean;
        };
        /**
         * @description An appearance every brand has.
         * @enum {string}
         */
        Appearance: "light" | "dark" | "high-contrast";
        /**
         * @description Who owns an assignment: an admin of this organization (`local`), or the identity provider,
         *     sync API, SCIM or connector that wrote it, in which case it cannot be removed here.
         * @enum {string}
         */
        AssignmentSource: "local" | "idp" | "sync" | "scim" | "connector";
        /** @description `AuthenticatorSelectionCriteria`. */
        AuthenticatorSelection: {
            authenticatorAttachment?: string;
            requireResidentKey: boolean;
            residentKey?: string;
            /** @description `required`: the authenticator checks a PIN or biometric. */
            userVerification: string;
        };
        /**
         * @description The organization's branding: the saved inputs (the default brand's when it has saved none),
         *     the generator version, the hash of the stylesheet `/theme.css` serves, and the contrast
         *     report of its light, dark and high-contrast themes.
         */
        Branding: {
            /** @description The SHA-256 of the stylesheet, lowercase hex. */
            css_hash: string;
            /**
             * Format: int32
             * @description The version of the theme generator that wrote the stylesheet.
             */
            generator: number;
            inputs: components["schemas"]["SavedInputs"];
            report: components["schemas"]["ContrastReport"][];
        };
        /**
         * @description The branding inputs. The whole set is replaced; an omitted or `null` choice takes its
         *     default. A value that is not a choice is a 422, not a 400.
         */
        BrandingInputs: {
            corner_style?: null | components["schemas"]["CornerStyle"];
            /** @description The primary color: a CSS hex color (`#500E0E`, `#f00`) or `oklch(L C H)`, opaque. */
            primary: string;
            reading_font?: null | components["schemas"]["ReadingFont"];
            ui_font?: null | components["schemas"]["UiFont"];
        };
        /**
         * @description A problem details body with the contrast report: `branding.contrast_failed` carries
         *     `report`, every other code leaves it out.
         */
        BrandingProblem: components["schemas"]["Problem"] & {
            /** @description On `branding.contrast_failed`, every appearance's report, failing pairs first. */
            report?: components["schemas"]["ContrastReport"][] | null;
        };
        /** @description One measured pair: `foreground` on `background`. */
        ContrastPair: {
            /** @description A color token, such as `bg-canvas`. */
            background: string;
            /** @description A color token, such as `fg-soft`. */
            foreground: string;
            /**
             * Format: double
             * @description 4.5 for text and 3 for other pairs; 7 and 4.5 in high contrast.
             */
            minimum: number;
            pass: boolean;
            /**
             * Format: double
             * @description The WCAG 2 contrast ratio on the displayed 8-bit sRGB colors, 1 to 21, unrounded.
             */
            ratio: number;
        };
        /** @description One appearance's contrast report. */
        ContrastReport: {
            appearance: components["schemas"]["Appearance"];
            pairs: components["schemas"]["ContrastPair"][];
            /** @description True when every pair passes. */
            pass: boolean;
            warnings: components["schemas"]["ContrastWarning"][];
        };
        /**
         * @description Something to look at that does not fail the theme. `brand-near-danger`: the brand's hue is
         *     within 30 degrees of `danger`'s.
         */
        ContrastWarning: {
            code: string;
            message: string;
        };
        /**
         * @description The corner style: `square` sets the `xs`, `sm` and `md` radii to 0, `round` doubles them.
         * @enum {string}
         */
        CornerStyle: "square" | "default" | "round";
        /** @description `PublicKeyCredentialDescriptorJSON`. */
        CredentialDescriptor: {
            /** @description The credential id, base64url, unpadded. */
            id: string;
            transports?: string[];
            type: string;
        };
        /** @description `PublicKeyCredentialParameters`. */
        CredentialParameter: {
            /**
             * Format: int32
             * @description A COSE algorithm identifier, such as -7 (ES256).
             */
            alg: number;
            type: string;
        };
        /**
         * @description The density a user chose. `compact` sets the shorter control heights.
         * @enum {string}
         */
        DensityChoice: "comfortable" | "compact";
        /**
         * @description A local method an enrollment link can set up.
         * @enum {string}
         */
        EnrollMethod: "passkey" | "password";
        /** @description An enrollment link, returned once: the only copy of its token. */
        EnrollmentLink: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            expires_at: string;
            /**
             * @description The link to hand to the user. It holds a secret: anyone with it can set up a way to
             *     sign in as the user until it is used, replaced or expires.
             */
            url: string;
        };
        /** @description The token from an enrollment link. */
        EnrollmentStart: {
            /** @description The text after `#token=` in the link. */
            token: string;
        };
        /** @description What an enrollment link sets up. */
        EnrollmentStarted: {
            /** @description The email the user will sign in with. */
            email: string;
            /** @description The methods the organization lets the user set up, passkey first. Never empty. */
            methods: components["schemas"]["EnrollMethod"][];
            /**
             * @description When `methods` lists `passkey`: the creation options for the passkey, bound to the
             *     sign-in flow whose cookie this answer sets. Finish with `POST
             *     /auth/local/enrollment/passkey` in this browser within 10 minutes.
             */
            passkey_options?: components["schemas"]["PasskeyCreationOptions"];
        };
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
        /** @description A user's identity as an admin sees it. */
        Identity: {
            /**
             * @description The attributes the provider sent at the latest sign-in, as mapped by the provider's
             *     configuration; `{}` before the first sign-in. Admission rules read them.
             */
            attributes: Record<string, never>;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /** @description The key of the identity provider. */
            provider_key: string;
            /** @description The provider's id for the person, exactly as the provider sends it. */
            subject: string;
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            updated_at: string;
        };
        /** @description The local sign-in method switches as the organization settings show them. */
        LocalMethods: {
            magic_link: boolean;
            passkey: boolean;
            password: boolean;
            totp: boolean;
        };
        /**
         * @description The organization's local sign-in: whether it is on at all, and each method's switch. A method
         *     is offered only when `enabled` and its own switch are both on.
         */
        LocalSignIn: {
            /** @description Local sign-in as a whole; off, no local method is offered. */
            enabled: boolean;
            /** @description Sign-in with a link sent by email. Off until the server can send email. */
            magic_link: boolean;
            /** @description Sign-in with a passkey. */
            passkey: boolean;
            /** @description Sign-in with a password. */
            password: boolean;
            /** @description A TOTP code as the second step after a password. */
            totp: boolean;
        };
        /** @description What `GET /api/v1/me` answers. */
        Me: {
            /** @description The theme and density the user chose (docs/specs/m0/11-app-shell.md B21, Q6). */
            appearance: components["schemas"]["UserAppearance"];
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
        /** @description One of the signed-in user's identities. */
        MyIdentity: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /** @description The key of the identity provider. */
            provider_key: string;
            /**
             * @description The provider's label, such as "Sign in with Example"; `null` when the provider's
             *     configuration has none.
             */
            provider_label: string | null;
            /** @description The provider's id for you, exactly as the provider sends it. */
            subject: string;
        };
        /** @description One of the signed-in user's passkeys. The credential itself never leaves the server. */
        MyPasskey: {
            /**
             * Format: date-time
             * @description RFC 3339, UTC.
             */
            created_at: string;
            /** Format: uuid */
            id: string;
            /**
             * Format: date-time
             * @description The last sign-in with it, RFC 3339, UTC; `null` before the first.
             */
            last_used_at: string | null;
            name: string;
        };
        /** @description A new name for a passkey. */
        MyPasskeyPatch: {
            /** @description Trimmed, 1 to 200 characters. */
            name: string;
        };
        /** @description The creation options for the browser. */
        MyPasskeyStarted: {
            /**
             * @description Pass `publicKey` to `navigator.credentials.create`, then send the result to `POST
             *     /api/v1/me/passkeys` in this browser within 10 minutes.
             */
            options: components["schemas"]["PasskeyCreationOptions"];
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
        /** @description An identity to link to a user, so they can sign in with it the first time. */
        NewIdentity: {
            /** @description The key of one of the organization's identity providers. */
            provider_key: string;
            /** @description The provider's id for the person; stored exactly as given, not empty. */
            subject: string;
        };
        /** @description The browser's registration response. */
        NewMyPasskey: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
            /** @description What you call this passkey: trimmed, 1 to 200 characters. `Passkey` when absent. */
            name?: string | null;
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
            /** @description Who admission lets in; read-only here. */
            admission: components["schemas"]["AdmissionSwitches"];
            /** @description The IANA time zone a user created without one gets; `UTC` unless set. */
            default_tz: string;
            /**
             * @description Whether a sign-in through an identity the organization does not know yet may be linked
             *     to the existing user with the same email address. `false` unless set, and this release
             *     refuses to set it to `true`.
             */
            email_matching: boolean;
            /**
             * @description The local sign-in method switches; read-only here, changed through
             *     `PUT /api/v1/admin/local-sign-in`.
             */
            local_sign_in: components["schemas"]["LocalMethods"];
        };
        /**
         * @description A partial change to the organization settings: one optional member per setting this route
         *     writes; only the members present change. A member that is no setting, or a setting another
         *     route writes, is refused.
         */
        OrgSettingsPatch: {
            /** @description An IANA time zone, matched in any letter case and stored in the database's spelling. */
            default_tz?: string;
            /**
             * @description Only `false` is accepted: `true` answers 422 with
             *     `settings.email_matching_unavailable`.
             */
            email_matching?: boolean;
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
        /**
         * @description `CredentialCreationOptions` as JSON: pass `publicKey` to
         *     `PublicKeyCredential.parseCreationOptionsFromJSON`, then to `navigator.credentials.create`.
         */
        PasskeyCreationOptions: {
            publicKey: components["schemas"]["PublicKeyCreationOptions"];
        };
        /** @description The browser's registration response for an enrollment link. */
        PasskeyEnrollment: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.create` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
            /** @description What the user calls this passkey: trimmed, 1 to 200 characters. `Passkey` when absent. */
            name?: string | null;
            /** @description The text after `#token=` in the link. */
            token: string;
        };
        /**
         * @description `CredentialRequestOptions` as JSON: pass `publicKey` to
         *     `PublicKeyCredential.parseRequestOptionsFromJSON`, then to `navigator.credentials.get`.
         */
        PasskeyRequestOptions: {
            publicKey: components["schemas"]["PublicKeyRequestOptions"];
        };
        /** @description The browser's answer to the request options. */
        PasskeySignInFinish: {
            /**
             * @description The `PublicKeyCredential` `navigator.credentials.get` returned, as JSON
             *     (`PublicKeyCredential.toJSON()`).
             */
            credential: Record<string, never>;
        };
        /** @description The address to sign in with. */
        PasskeySignInStart: {
            /** @description The account's email, in any letter case. */
            email: string;
            /** @description The path on this site to open after signing in; anything else becomes `/`. */
            return_to?: string | null;
        };
        /** @description The request options for the browser. */
        PasskeySignInStarted: {
            /**
             * @description Pass to `navigator.credentials.get`, then send the result to `POST
             *     /auth/local/passkey/finish` in this browser within 10 minutes.
             */
            options: components["schemas"]["PasskeyRequestOptions"];
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
        /** @description `PublicKeyCredentialCreationOptionsJSON`. */
        PublicKeyCreationOptions: {
            /** @description `none`: the server does not ask which device this is. */
            attestation?: string;
            attestationFormats?: string[];
            authenticatorSelection?: components["schemas"]["AuthenticatorSelection"];
            /** @description base64url, unpadded. */
            challenge: string;
            /** @description The user's registered passkeys, which the authenticator must not register again. */
            excludeCredentials?: components["schemas"]["CredentialDescriptor"][];
            extensions?: components["schemas"]["RegistrationExtensions"];
            hints?: string[];
            pubKeyCredParams: components["schemas"]["CredentialParameter"][];
            rp: components["schemas"]["RelyingPartyInfo"];
            /**
             * Format: int32
             * @description Milliseconds.
             */
            timeout?: number;
            user: components["schemas"]["UserInfo"];
        };
        /** @description `PublicKeyCredentialRequestOptionsJSON`. */
        PublicKeyRequestOptions: {
            /**
             * @description The passkeys the account may sign in with. Empty when the address has no account or
             *     the account has no passkey: the answer looks the same either way.
             */
            allowCredentials: components["schemas"]["CredentialDescriptor"][];
            /** @description base64url, unpadded. */
            challenge: string;
            /** @description The relying party ID: the instance's host name. */
            rpId: string;
            /**
             * Format: int32
             * @description Milliseconds.
             */
            timeout?: number;
            /** @description `required`: the authenticator checks a PIN or biometric. */
            userVerification: string;
        };
        /**
         * @description The reading font: the bundled serif, or the organization's UI font.
         * @enum {string}
         */
        ReadingFont: "serif" | "sans";
        /** @description `AuthenticationExtensionsClientInputs` for a registration. */
        RegistrationExtensions: {
            credProps?: boolean;
            credentialProtectionPolicy?: string;
            enforceCredentialProtectionPolicy?: boolean;
            hmacCreateSecret?: boolean;
            minPinLength?: boolean;
            uvm?: boolean;
        };
        /** @description `PublicKeyCredentialRpEntity`. */
        RelyingPartyInfo: {
            /** @description The relying party ID: the instance's host name. */
            id: string;
            /** @description The organization's name. */
            name: string;
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
        /** @description The saved inputs, every choice made. */
        SavedInputs: {
            corner_style: components["schemas"]["CornerStyle"];
            primary: string;
            reading_font: components["schemas"]["ReadingFont"];
            ui_font: components["schemas"]["UiFont"];
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
        /** @description A completed sign-in. */
        SignedIn: {
            /** @description The path on this site to open next. */
            return_to: string;
        };
        /**
         * @description The theme a user chose. `system` follows the operating system's color and contrast settings.
         * @enum {string}
         */
        ThemeChoice: "system" | "light" | "dark" | "high-contrast";
        /**
         * @description A UI font the instance bundles.
         * @enum {string}
         */
        UiFont: "Atkinson Hyperlegible Next" | "Roboto";
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
        /** @description The theme and density the signed-in user's pages are drawn in. */
        UserAppearance: {
            density: components["schemas"]["DensityChoice"];
            theme: components["schemas"]["ThemeChoice"];
        };
        /** @description `PublicKeyCredentialUserEntityJSON`. */
        UserInfo: {
            displayName: string;
            /** @description The user handle, base64url, unpadded: the user's id. */
            id: string;
            /** @description The user's email. */
            name: string;
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
        /** @description `branding.invalid_input`: an input fails validation; `errors` names it (`branding.invalid_color`, `branding.too_long`, `branding.not_a_choice`). `branding.contrast_failed`: a generated theme has a failing contrast pair; `report` lists every appearance's pairs, failing pairs first. */
        BrandingInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["BrandingProblem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `branding.manage` at organization scope. `entitlement.feature_unavailable`: the organization's plan does not include `custom_branding`. `auth.csrf_refused`: the request did not come from this site. */
        BrandingWriteForbidden: {
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
        /** @description `auth.suspended`: the account is suspended. `auth.csrf_refused`: the request did not come from this site. */
        EnrollPasskeyForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{token, credential, name?}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime. `auth.flow_invalid`: no registration was started in this browser for this link, or it was already finished; open the link again. `auth.flow_expired`: the registration was started more than 10 minutes ago; open the link again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; open the link again and retry. */
        EnrollPasskeyInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        EnrollPasskeyNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.email_required`: the user has no email address, which local sign-in finds them by; add one first. */
        EnrollmentLinkConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{token}`. `auth.enrollment_invalid`: the link is unknown, already used, replaced by a newer link, or for another organization. `auth.enrollment_expired`: the link is older than its 7-day lifetime; ask an admin for a new one. */
        EnrollmentStartInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: the organization offers neither passkey nor password sign-in. `org.not_found`: the request's organization cannot be determined. */
        EnrollmentStartNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.last_sign_in_method`: the user would be left with no way to sign in. `identity.local`: the identity is the user's local account, which is not removed here. */
        IdentityDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `user.not_found`: no user of the organization has this id. `identity.not_found`: the user has no identity with this id. `org.not_found`: the request's organization cannot be determined. */
        IdentityDeleteNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.taken`: this provider's subject is already linked to a user. */
        IdentityLinkConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `user.manage` at organization scope. `permission.escalation`: the user holds a permission, at any scope, that the caller does not hold at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        IdentityWriteForbidden: {
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
        /** @description `local.last_method`: the change would leave the organization no way to sign in: no offered passkey, password or magic link (TOTP is only a second step), and no enabled outside provider. */
        LocalSignInConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `permission.denied`: the caller does not hold `provider.manage` at organization scope. `permission.escalation`: the change affects how every user signs in, and the caller does not hold every catalog permission at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        LocalSignInForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.invalid`: the change breaks a rule; `errors` lists each one. `local.magic_link_needs_email` marks `magic_link` switched on while the server cannot send email. */
        LocalSignInInvalid: {
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
        /** @description `identity.last_sign_in_method`: you would be left with no way to sign in. `identity.local`: the identity is your local account, which is not removed here. `identity.externally_managed`: an identity provider, the sync API, SCIM or a connector owns your user record and would link the identity again; ask an admin. */
        MyIdentityDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `identity.not_found`: you have no identity with this id. `org.not_found`: the request's organization cannot be determined. */
        MyIdentityNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `local.passkeys_not_allowed`: you have no local account, and no identity provider you sign in with lets its users add a passkey. `auth.csrf_refused`: the request did not come from this site. */
        MyPasskeyAddForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `request.malformed`: the body is not `{credential, name?}`. `auth.flow_invalid`: no passkey registration was started in this browser for you, or it was already finished; start again. `auth.flow_expired`: the registration was started more than 10 minutes ago; start again. `auth.passkey_invalid`: the passkey's response did not verify, or this passkey is already registered; start again and retry. */
        MyPasskeyAddInvalid: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `local.last_method`: this passkey is your last way to sign in. Add another passkey before you delete this one. */
        MyPasskeyDeleteConflict: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `passkey.not_found`: you have no passkey with this id. `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        MyPasskeyNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. `auth.csrf_refused`: the request did not come from this site. */
        MyPasskeyWriteForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.session_required`: the request carries a bearer credential; only a signed-in browser session manages passkeys. */
        MyPasskeysForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        MyPasskeysNotFound: {
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
        /** @description `request.malformed`: the body is not `{credential}`. `auth.flow_invalid`: no passkey sign-in was started in this browser, or it was already finished; start again. `auth.flow_expired`: the sign-in was started more than 10 minutes ago; start again. */
        PasskeySignInFinishBadRequest: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.suspended`: the account is suspended. `local.passkeys_not_allowed`: the account has no local account and no identity provider it signs in with allows passkeys, so its passkeys do not sign in. `auth.csrf_refused`: the request did not come from this site. */
        PasskeySignInFinishForbidden: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        PasskeySignInFinishNotFound: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.passkey_invalid`: the passkey did not sign in: it is not a passkey of the account the sign-in was started for, or its answer did not verify. Start again. */
        PasskeySignInFinishUnauthorized: {
            headers: {
                [name: string]: unknown;
            };
            content: {
                "application/problem+json": components["schemas"]["Problem"];
            };
        };
        /** @description `auth.method_disabled`: passkey sign-in is switched off for this organization. `org.not_found`: the request's organization cannot be determined. */
        PasskeySignInStartNotFound: {
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
        /** @description `request.invalid`: the patch breaks a rule; `errors` lists each one. `settings.unknown_key` marks a key that is no organization setting; `settings.read_only` a setting this route does not change (`local_sign_in`: `PUT /api/v1/admin/local-sign-in`; `admission`: the admission settings); `tz.unknown` a `default_tz` that is no IANA time zone; `settings.email_matching_unavailable` an `email_matching` of `true`, which this release refuses. */
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
    getBranding: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's branding */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Branding"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateBranding: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["BrandingInputs"];
            };
        };
        responses: {
            /** @description The organization's branding after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Branding"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["BrandingWriteForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["BrandingInvalid"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    getLocalSignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The local sign-in switches */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LocalSignIn"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["PermissionDenied"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateLocalSignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["LocalSignIn"];
            };
        };
        responses: {
            /** @description The switches after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LocalSignIn"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["LocalSignInForbidden"];
            404: components["responses"]["OrganizationNotFound"];
            409: components["responses"]["LocalSignInConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["LocalSignInInvalid"];
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
    createEnrollmentLink: {
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
            /** @description The link, shown once */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnrollmentLink"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["IdentityWriteForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["EnrollmentLinkConflict"];
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
    updateMyAppearance: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UserAppearance"];
            };
        };
        responses: {
            /** @description The appearance after the change */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UserAppearance"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["OrganizationNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listMyIdentities: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Your identities */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyIdentity"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteMyIdentity: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The identity's id. */
                identity_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The identity is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["MyIdentityNotFound"];
            409: components["responses"]["MyIdentityDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    listMyPasskeys: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Your passkeys */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"][];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeysForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    createMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["NewMyPasskey"];
            };
        };
        responses: {
            /** @description The passkey is added */
            201: {
                headers: {
                    /** @description `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"];
                };
            };
            400: components["responses"]["MyPasskeyAddInvalid"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyAddForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The creation options; the registration is waiting in this browser */
            200: {
                headers: {
                    /** @description The `__Host-sc_flow` cookie of the registration */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskeyStarted"];
                };
            };
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyAddForbidden"];
            404: components["responses"]["MyPasskeysNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The passkey's id. */
                passkey_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The passkey is deleted */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyWriteForbidden"];
            404: components["responses"]["MyPasskeyNotFound"];
            409: components["responses"]["MyPasskeyDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    updateMyPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The passkey's id. */
                passkey_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["MyPasskeyPatch"];
            };
        };
        responses: {
            /** @description The passkey with its new name */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MyPasskey"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["MyPasskeyWriteForbidden"];
            404: components["responses"]["MyPasskeyNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
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
    listUserIdentities: {
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
            /** @description The user's identities */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Identity"][];
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
    createUserIdentity: {
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
                "application/json": components["schemas"]["NewIdentity"];
            };
        };
        responses: {
            /** @description The identity is linked */
            201: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["Identity"];
                };
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["IdentityWriteForbidden"];
            404: components["responses"]["UserNotFound"];
            409: components["responses"]["IdentityLinkConflict"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    deleteUserIdentity: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description The user's id. */
                user_id: string;
                /** @description The identity's id. */
                identity_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The identity is removed */
            204: {
                headers: {
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            401: components["responses"]["Unauthenticated"];
            403: components["responses"]["IdentityWriteForbidden"];
            404: components["responses"]["IdentityDeleteNotFound"];
            409: components["responses"]["IdentityDeleteConflict"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    enrollPasskey: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeyEnrollment"];
            };
        };
        responses: {
            /** @description The passkey is registered and the user is signed in */
            200: {
                headers: {
                    /** @description The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["EnrollPasskeyInvalid"];
            403: components["responses"]["EnrollPasskeyForbidden"];
            404: components["responses"]["EnrollPasskeyNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            422: components["responses"]["UnprocessableEntity"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startEnrollment: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["EnrollmentStart"];
            };
        };
        responses: {
            /** @description The link is live */
            200: {
                headers: {
                    /** @description `no-store`: the answer holds the user's email */
                    "Cache-Control"?: string;
                    /** @description With `passkey_options`: the `__Host-sc_flow` cookie of the passkey registration */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["EnrollmentStarted"];
                };
            };
            400: components["responses"]["EnrollmentStartInvalid"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["EnrollmentStartNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    finishPasskeySignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeySignInFinish"];
            };
        };
        responses: {
            /** @description The user is signed in */
            200: {
                headers: {
                    /** @description The new `__Host-sc_session` cookie, and `__Host-sc_flow` cleared */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SignedIn"];
                };
            };
            400: components["responses"]["PasskeySignInFinishBadRequest"];
            401: components["responses"]["PasskeySignInFinishUnauthorized"];
            403: components["responses"]["PasskeySignInFinishForbidden"];
            404: components["responses"]["PasskeySignInFinishNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startPasskeySignIn: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PasskeySignInStart"];
            };
        };
        responses: {
            /** @description The request options, whether or not the address has an account or a passkey */
            200: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The `__Host-sc_flow` cookie of the sign-in */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PasskeySignInStarted"];
                };
            };
            400: components["responses"]["BadRequest"];
            403: components["responses"]["CsrfRefused"];
            404: components["responses"]["PasskeySignInStartNotFound"];
            413: components["responses"]["PayloadTooLarge"];
            415: components["responses"]["UnsupportedMediaType"];
            429: components["responses"]["RateLimited"];
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
    finishProviderSignIn: {
        parameters: {
            query?: {
                /** @description The authorization code, on success. */
                code?: string;
                /** @description The `state` the start route sent. */
                state?: string;
                /** @description The provider's error code, on failure. */
                error?: string;
                /** @description The provider's description of the error. Logged, never shown. */
                error_description?: string;
            };
            header?: never;
            path: {
                /** @description The provider's key in the organization (`identity_providers.key`). */
                provider: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description To the flow's `return_to`, signed in, or to `/sign-in?error=<code>` */
            303: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The `return_to` path, or the sign-in page with the error code */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    /** @description Clears the `__Host-sc_flow` cookie; signed in, also sets `__Host-sc_session` */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
            503: components["responses"]["NoOrganization"];
        };
    };
    startProviderSignIn: {
        parameters: {
            query?: {
                /**
                 * @description The path on this site to land on after sign-in. Anything but a safe path on this host
                 *     becomes `/`.
                 */
                return_to?: string;
            };
            header?: never;
            path: {
                /** @description The provider's key in the organization (`identity_providers.key`). */
                provider: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description To the provider's authorization endpoint, or to `/sign-in?error=<code>` */
            303: {
                headers: {
                    /** @description `no-store` */
                    "Cache-Control"?: string;
                    /** @description The authorization URL, or the sign-in page with the error code */
                    Location?: string;
                    /** @description With `auth.rate_limited`: seconds to wait, at least 1 */
                    "Retry-After"?: number;
                    /** @description To the provider: the `__Host-sc_flow` cookie of the sign-in flow */
                    "Set-Cookie"?: string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            400: components["responses"]["BadRequest"];
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
    getThemeCss: {
        parameters: {
            query?: {
                /**
                 * @description The stylesheet's SHA-256 (lowercase hex), as the served page links it. When it is the
                 *     served stylesheet's, the answer is cached for a year; otherwise it is revalidated on
                 *     every load.
                 */
                v?: string;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description The organization's light, dark and high-contrast themes as CSS variables: the default brand's when it has saved no branding or the instance has no organization yet. Cached for a year (`Cache-Control: public, max-age=31536000, immutable`) when `v` is the stylesheet's hash, as the page the server serves links it; revalidated on every load (`no-cache`) otherwise. */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "text/css": string;
                };
            };
            400: components["responses"]["BadRequest"];
            404: components["responses"]["OrganizationNotFound"];
            500: components["responses"]["InternalError"];
        };
    };
}
