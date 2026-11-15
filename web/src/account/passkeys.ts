// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-in user's passkeys through `/api/v1/me/passkeys` (docs/specs/m0/04-local-accounts.md
// B13): list, add (the start route's options, the browser's ceremony, then the add route), rename
// and delete. The server decides every refusal; these only carry its answer.
import type { components } from "@stepclimb/api-client";
import { queryOptions } from "@tanstack/react-query";
import { api, unwrap } from "../api";
import { createPasskey } from "../sign-in/passkey";

export type MyPasskey = components["schemas"]["MyPasskey"];
type CreationOptions = components["schemas"]["PasskeyCreationOptions"];

export const myPasskeysQuery = queryOptions({
  queryKey: ["me", "passkeys"],
  queryFn: async (): Promise<MyPasskey[]> => unwrap(await api.GET("/api/v1/me/passkeys")),
});

/** Starts a registration in this browser: the creation options, bound to a new flow cookie. */
export async function startAddPasskey(): Promise<CreationOptions> {
  return unwrap(await api.POST("/api/v1/me/passkeys/start")).options;
}

/**
 * Runs the browser's ceremony with `options`, then sends the credential with `name`. A blank name
 * is left out, so the server gives the passkey its default name.
 */
export async function addPasskey(options: CreationOptions, name: string): Promise<MyPasskey> {
  const credential = await createPasskey(options);
  const body = name.trim() === "" ? { credential } : { credential, name };
  return unwrap(await api.POST("/api/v1/me/passkeys", { body }));
}

export async function renamePasskey(id: string, name: string): Promise<MyPasskey> {
  const params = { path: { passkey_id: id } };
  return unwrap(await api.PATCH("/api/v1/me/passkeys/{passkey_id}", { params, body: { name } }));
}

export async function deletePasskey(id: string): Promise<void> {
  const params = { path: { passkey_id: id } };
  unwrap(await api.DELETE("/api/v1/me/passkeys/{passkey_id}", { params }));
}
