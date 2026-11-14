// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-in user and the session's end (docs/specs/m0/11-app-shell.md B9 to B12, B15): the
// `/me` query, the return target carried to the sign-in screen, and the query client whose
// every failed query or mutation reports a 401 to one handler.
import type { components } from "@stepclimb/api-client";
import { MutationCache, QueryCache, QueryClient, queryOptions } from "@tanstack/react-query";
import { api, isUnauthorized, unwrap } from "./api";

export type Me = components["schemas"]["Me"];

export const meQuery = queryOptions({
  queryKey: ["me"],
  queryFn: async (): Promise<Me> => unwrap(await api.GET("/api/v1/me")),
});

/** Ends the browser's session. The server answers 204 whether or not the cookie named one. */
export async function signOut(): Promise<void> {
  unwrap(await api.POST("/auth/sign-out"));
}

/** The longest return target kept, as the server's flows keep it (spec 08 B19). */
export const MAX_RETURN_TARGET = 2048;

/**
 * `value` when it is a path on this origin, otherwise `/` (B15). The rule is the server's own
 * (spec 08 B19): one leading `/`, not followed by `/` or `\`, only printable ASCII without
 * spaces, at most 2,048 characters. Browsers drop tabs and newlines from a URL, so `/<tab>/x`
 * would reach them as `//x`; refusing anything outside `!` to `~` closes that.
 */
export function safeReturnTarget(value: unknown): string {
  if (typeof value !== "string" || value.length > MAX_RETURN_TARGET) return "/";
  if (!value.startsWith("/") || value[1] === "/" || value[1] === "\\") return "/";
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code < 0x21 || code > 0x7e) return "/";
  }
  return value;
}

/**
 * The client every query and mutation runs through. A 401 from any of them calls
 * `onUnauthorized`, which decides whether the session ended (B10). Failed requests are not
 * retried: the views offer "Try again", and a 401 must reach the handler at once.
 */
export function createQueryClient(onUnauthorized: () => void): QueryClient {
  const onError = (error: unknown) => {
    if (isUnauthorized(error)) onUnauthorized();
  };
  return new QueryClient({
    queryCache: new QueryCache({ onError }),
    mutationCache: new MutationCache({ onError }),
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
}

/** True when `me` holds `permission` at any scope (B5). */
export function holdsPermission(me: Me, permission: string): boolean {
  return me.grants.some((grant) => grant.permissions.includes(permission));
}
