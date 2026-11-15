// SPDX-License-Identifier: AGPL-3.0-or-later
// The organization's discovery document (docs/specs/m0/08-sessions-and-csrf.md B38): its name and
// the sign-in methods it offers, which the signed-out screens read before anyone has signed in
// (11-app-shell.md B13). The server decides what is offered; these helpers only sort its answer
// into what the screens draw.
import type { components } from "@stepclimb/api-client";
import { queryOptions, useQuery } from "@tanstack/react-query";
import { api, unwrap } from "../api";

export type Discovery = components["schemas"]["Discovery"];
export type SignInMethod = components["schemas"]["SignInMethod"];
export type LocalMethod = components["schemas"]["LocalMethod"];

/**
 * Read once per page load and kept for a minute, so the layout and the step views, and the steps
 * one after another, share one answer. A page load always asks again: an administrator's change
 * shows on the next visit.
 */
export const discoveryQuery = queryOptions({
  queryKey: ["discovery"],
  queryFn: async (): Promise<Discovery> => unwrap(await api.GET("/.well-known/stepclimb")),
  staleTime: 60_000,
});

/** The organization's name once the document has loaded, for the header and document title. */
export function useOrganizationName(): string | undefined {
  return useQuery({ ...discoveryQuery, select: (document) => document.name }).data;
}

/**
 * The local methods the browser can sign in with, in the document's order. The email step leads
 * to a passkey or a password; TOTP is a second step after a password and the magic link has no
 * screen yet, so neither is a way in on its own.
 */
export function emailMethods(document: Discovery): LocalMethod[] {
  const local = document.sign_in.find((method) => method.kind === "local");
  return (local?.methods ?? []).filter((method) => method === "passkey" || method === "password");
}

/** The outside providers, in the document's order. */
export function providers(document: Discovery): SignInMethod[] {
  return document.sign_in.filter((method) => method.kind !== "local");
}

/**
 * The provider's start route (05-oidc, 06-oauth2-and-vatsim-preset), opened as a full page so the
 * provider's redirect and the session cookie work as the browser expects (B14). The return target
 * rides along for the server to check again (B15).
 */
export function providerStartHref(key: string, returnTo: string | undefined): string {
  const path = `/auth/${encodeURIComponent(key)}/start`;
  return returnTo === undefined ? path : `${path}?return_to=${encodeURIComponent(returnTo)}`;
}
