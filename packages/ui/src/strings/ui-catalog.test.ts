// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI's own catalog: every id its components use exists, every message is valid ICU,
// no message hard-codes an overridable noun
// or puts an article before a term, and every lookup uses a literal id.
import { describe, expect, it } from "vitest";
import { uiMessages } from "./catalog";
import {
  articlesBeforeTerms,
  dynamicLookups,
  hardCodedNouns,
  invalidMessages,
  missingMessageIds,
} from "./check";

const sources = import.meta.glob<string>(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
});

describe("Stepclimb UI catalog", () => {
  it("walks the package's source", () => {
    expect(Object.keys(sources)).toContain("../primitives/button.tsx");
  });

  it("has every message id the components use", () => {
    expect(missingMessageIds(sources, [uiMessages])).toEqual([]);
  });

  it("looks every message up by a literal id", () => {
    expect(dynamicLookups(sources)).toEqual([]);
  });

  it("holds only valid ICU messages", () => {
    expect(invalidMessages([uiMessages])).toEqual([]);
  });

  it("writes terms as arguments, never as text", () => {
    expect(hardCodedNouns([uiMessages])).toEqual([]);
  });

  it("puts no article directly before a term", () => {
    expect(articlesBeforeTerms([uiMessages])).toEqual([]);
  });
});
