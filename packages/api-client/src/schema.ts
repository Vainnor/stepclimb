// SPDX-License-Identifier: Apache-2.0
// Generated from openapi/stepclimb.json by scripts/generate.mjs. Do not edit; run
// `just openapi` instead.
export interface paths {
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
    };
    responses: {
        /** @description `request.malformed`: the body, a path parameter or a query parameter does not parse or does not match the schema. `pagination.invalid_cursor`: the cursor does not decode. */
        BadRequest: {
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
        /** @description `request.too_large`: the body is larger than the server accepts. */
        PayloadTooLarge: {
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
    };
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
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
