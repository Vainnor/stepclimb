// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { termArguments } from "./terms";

// A term starting with "i", whose capital differs in Turkish (dotted İ) and English (I).
const terms = { item: { one: "item", other: "items" } };

describe("termArguments", () => {
  it("names arguments in English and capitalizes values in the active locale", () => {
    const args = termArguments(terms, { item: { one: "istasyon", other: "istasyonlar" } }, "tr");
    expect(args).toEqual({
      item: "istasyon",
      items: "istasyonlar",
      Item: "İstasyon",
      Items: "İstasyonlar",
    });
  });

  it("keeps the defaults without an override, and never lower-cases", () => {
    expect(termArguments(terms, {}, "en")).toEqual({
      item: "item",
      items: "items",
      Item: "Item",
      Items: "Items",
    });
    const args = termArguments(terms, { item: { one: "LOA item", other: "LOA items" } }, "en");
    expect(args.item).toBe("LOA item");
    expect(args.Item).toBe("LOA item");
  });
});
