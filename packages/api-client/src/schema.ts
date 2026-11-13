// SPDX-License-Identifier: Apache-2.0
// Generated from openapi/stepclimb.json by scripts/generate.mjs. Do not edit; run
// `just openapi` instead.
export interface paths {
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
        /** @description One rule a request body broke, in a 422's `errors`. */
        FieldError: {
            /** @description The stable code of the rule, such as `settings.unknown_key`. */
            code: string;
            /** @description A JSON Pointer (RFC 6901) to the member that broke the rule, such as `/name`. */
            field: string;
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
             * @description Catalog keys (`GET /api/v1/admin/permissions`), each held by the caller at organization
             *     scope.
             */
            permissions: string[];
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
        /** @description One permission of the catalog. */
        PermissionInfo: {
            /** @description The scope types at which a role assignment grants it. */
            grantable: components["schemas"]["ScopeType"][];
            /** @description The key roles list, `area.action`. */
            key: string;
        };
        /** @description The body of every error response (RFC 9457). */
        Problem: {
            /** @description The stable error code, `area.reason`. */
            code: string;
            /** @description A sentence for a human reader. Clients branch on `code`, never on this text. */
            detail: string;
            /** @description On a 422, each rule the body broke. */
            errors?: components["schemas"]["FieldError"][];
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
        /** @description A change to a custom role; an omitted member is left as it is. */
        RolePatch: {
            /** @description Trimmed; 1 to 200 characters. */
            name?: string | null;
            /**
             * @description The role's whole new list. The caller must hold at organization scope every key it adds
             *     and every key it drops.
             */
            permissions?: string[] | null;
        };
        /**
         * @description The kind of scope a permission can take effect at.
         * @enum {string}
         */
        ScopeType: "organization" | "account" | "course";
    };
    responses: {
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
        /** @description `permission.denied`: the caller does not hold `role.manage` at organization scope. `auth.csrf_refused`: the request did not come from this site. */
        DeleteForbidden: {
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
        /** @description `request.method_not_allowed`: the path does not take this method; `Allow` lists the methods it takes. */
        MethodNotAllowed: {
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
        /** @description `org.not_found`: the request's organization cannot be determined. */
        OrganizationNotFound: {
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
        /** @description `permission.denied`: the caller does not hold `role.manage` at organization scope. `permission.escalation`: the caller does not hold, at organization scope, a permission the role gains or loses. `auth.csrf_refused`: the request did not come from this site. */
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
