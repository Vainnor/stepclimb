// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearDraft, clearOwnerDrafts, readDraft, writeDraft } from "./draft";

const body = { type: "doc" as const, content: [{ type: "paragraph" }] };
const page = "01900000-0000-7000-8000-0000000000c1";
const base = "01900000-0000-7000-8000-0000000000d1";
const avery = { orgId: "org-a", userId: "user-1" };
const blake = { orgId: "org-a", userId: "user-2" };
const elsewhere = { orgId: "org-b", userId: "user-1" };

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("page drafts", () => {
  it("keep the body with the version it was based on, per user and page", () => {
    expect(writeDraft(avery, page, base, body, 1_000)).toBe(true);
    expect(readDraft(avery, page)).toEqual({ base_version_id: base, body, saved_at: 1_000 });
    expect(readDraft(avery, "another-page")).toBeUndefined();
    expect(readDraft(blake, page)).toBeUndefined();
    expect(readDraft(elsewhere, page)).toBeUndefined();
    clearDraft(avery, page);
    expect(readDraft(avery, page)).toBeUndefined();
  });

  it("clears every draft of one user and no one else's", () => {
    writeDraft(avery, page, base, body, 1);
    writeDraft(avery, "page-2", base, body, 1);
    writeDraft(blake, page, base, body, 1);
    writeDraft(elsewhere, page, base, body, 1);
    clearOwnerDrafts(avery);
    expect(readDraft(avery, page)).toBeUndefined();
    expect(readDraft(avery, "page-2")).toBeUndefined();
    expect(readDraft(blake, page)).toBeDefined();
    expect(readDraft(elsewhere, page)).toBeDefined();
  });

  it("ignore a stored value that is not a draft", () => {
    localStorage.setItem(`stepclimb.page-draft.org-a.user-1.${page}`, "{not json");
    expect(readDraft(avery, page)).toBeUndefined();
    localStorage.setItem(
      `stepclimb.page-draft.org-a.user-1.${page}`,
      JSON.stringify({ body: "x" }),
    );
    expect(readDraft(avery, page)).toBeUndefined();
  });

  it("report a draft the storage refused", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("full", "QuotaExceededError");
    });
    expect(writeDraft(avery, page, base, body, 1)).toBe(false);
  });
});
