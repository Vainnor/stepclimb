// SPDX-License-Identifier: AGPL-3.0-or-later
// The web app's catalog: every id the app uses exists in Stepclimb UI's or the app's catalog,
// every message is valid ICU, no message hard-codes an overridable noun
// or puts an article before a term, and every lookup uses a literal id.
import {
  articlesBeforeTerms,
  dynamicLookups,
  hardCodedNouns,
  invalidMessages,
  missingMessageIds,
} from "@stepclimb/ui/strings-check";
import { describe, expect, it } from "vitest";
import { catalogs } from "./messages";

const sources = import.meta.glob<string>(["./**/*.{ts,tsx}", "!./**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
});

describe("web catalog", () => {
  it("walks the app's source", () => {
    expect(Object.keys(sources)).toContain("./routes/index.tsx");
  });

  it("has every message id the app uses", () => {
    expect(missingMessageIds(sources, catalogs)).toEqual([]);
  });

  it("looks every message up by a literal id", () => {
    expect(dynamicLookups(sources)).toEqual([]);
  });

  it("holds only valid ICU messages", () => {
    expect(invalidMessages(catalogs)).toEqual([]);
  });

  it("writes terms as arguments, never as text", () => {
    expect(hardCodedNouns(catalogs)).toEqual([]);
  });

  it("puts no article directly before a term", () => {
    expect(articlesBeforeTerms(catalogs)).toEqual([]);
  });
});
