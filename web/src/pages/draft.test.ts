// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearDraft, readDraft, writeDraft } from "./draft";

const body = { type: "doc" as const, content: [{ type: "paragraph" }] };
const page = "01900000-0000-7000-8000-0000000000c1";
const base = "01900000-0000-7000-8000-0000000000d1";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("page drafts", () => {
  it("keep the body with the version it was based on, per page", () => {
    expect(writeDraft(page, base, body, 1_000)).toBe(true);
    expect(readDraft(page)).toEqual({ base_version_id: base, body, saved_at: 1_000 });
    expect(readDraft("another-page")).toBeUndefined();
    clearDraft(page);
    expect(readDraft(page)).toBeUndefined();
  });

  it("ignore a stored value that is not a draft", () => {
    localStorage.setItem(`stepclimb.page-draft.${page}`, "{not json");
    expect(readDraft(page)).toBeUndefined();
    localStorage.setItem(`stepclimb.page-draft.${page}`, JSON.stringify({ body: "x" }));
    expect(readDraft(page)).toBeUndefined();
  });

  it("report a draft the storage refused", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("full", "QuotaExceededError");
    });
    expect(writeDraft(page, base, body, 1)).toBe(false);
  });
});
