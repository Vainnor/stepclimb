// SPDX-License-Identifier: AGPL-3.0-or-later
// Unsaved work in the page editor, kept in this browser's local storage
// (docs/specs/m1/07-pages-and-the-editor.md B4): one draft per page, recorded with the version it
// was based on, so reopening the page offers it back and a save of it still meets B3's conflict
// check. A draft is the signed-in user's in their organization: another person signed in on the
// same browser is never offered it, and signing out clears it (an expired session keeps it, so
// the work comes back after signing in again). Storage that is full or turned off keeps nothing,
// and the editor works without it.
import type { PageDoc } from "@stepclimb/ui/editor";

export interface PageDraft {
  /** The version the author was editing when they made these changes. */
  base_version_id: string;
  body: PageDoc;
  /** When it was written, as milliseconds since the epoch. */
  saved_at: number;
}

/** Whose drafts: the organization and user of `/me`. */
export interface DraftOwner {
  orgId: string;
  userId: string;
}

const PREFIX = "stepclimb.page-draft.";
const ownerPrefix = (owner: DraftOwner) => `${PREFIX}${owner.orgId}.${owner.userId}.`;
const key = (owner: DraftOwner, pageId: string) => `${ownerPrefix(owner)}${pageId}`;

function isDraft(value: unknown): value is PageDraft {
  if (typeof value !== "object" || value === null) return false;
  const draft = value as Record<string, unknown>;
  const body = draft.body as Record<string, unknown> | null | undefined;
  return (
    typeof draft.base_version_id === "string" &&
    typeof draft.saved_at === "number" &&
    typeof body === "object" &&
    body !== null &&
    body.type === "doc" &&
    Array.isArray(body.content)
  );
}

function storage(): Storage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    // A browser that refuses storage to the page (a privacy setting) throws on access.
    return undefined;
  }
}

/** The page's draft, or `undefined` for none or one that does not parse. */
export function readDraft(owner: DraftOwner, pageId: string): PageDraft | undefined {
  const text = storage()?.getItem(key(owner, pageId));
  if (!text) return undefined;
  try {
    const value: unknown = JSON.parse(text);
    return isDraft(value) ? value : undefined;
  } catch {
    return undefined;
  }
}

/** Keeps `body`, based on `base`, as the page's draft. Returns false when storage refused it. */
export function writeDraft(
  owner: DraftOwner,
  pageId: string,
  base: string,
  body: PageDoc,
  now: number,
): boolean {
  const draft: PageDraft = { base_version_id: base, body, saved_at: now };
  try {
    storage()?.setItem(key(owner, pageId), JSON.stringify(draft));
    return storage() !== undefined;
  } catch {
    // Over the storage quota: the draft is not kept; the author can still save.
    return false;
  }
}

export function clearDraft(owner: DraftOwner, pageId: string): void {
  storage()?.removeItem(key(owner, pageId));
}

/** Removes every draft of `owner`: on an explicit sign-out. */
export function clearOwnerDrafts(owner: DraftOwner): void {
  const store = storage();
  if (!store) return;
  const prefix = ownerPrefix(owner);
  const keys = Array.from({ length: store.length }, (_, i) => store.key(i)).filter(
    (name): name is string => name?.startsWith(prefix) ?? false,
  );
  for (const name of keys) store.removeItem(name);
}

/** The draft owner of a `/me` answer. */
export function draftOwner(me: { organization: { id: string }; user: { id: string } }): DraftOwner {
  return { orgId: me.organization.id, userId: me.user.id };
}
