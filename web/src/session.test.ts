// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { MAX_RETURN_TARGET, safeReturnTarget } from "./session";

describe("safeReturnTarget (B15, T7)", () => {
  it.each([
    "//evil.example",
    "https://evil.example/x",
    "/\\evil.example",
    "javascript:alert(1)",
    "/\t/evil.example",
    "/\n/evil.example",
    "/a b",
    "/café",
    "",
    "courses",
  ])("sends %j to /", (value) => {
    expect(safeReturnTarget(value)).toBe("/");
  });

  it.each([
    "/..//evil.example",
    "/.//evil.example",
    "/a/..//evil",
    "/%2e%2e//evil.example",
    "/%2E//evil.example",
    "/a/.%2e//evil",
    "/a/%2e./b",
    "/a\\..\\/evil",
    "/a/.",
    "/a/..",
    "/a/..?x=1",
  ])("refuses the dot segment in %j", (value) => {
    expect(safeReturnTarget(value)).toBe("/");
  });

  it.each([
    "/",
    "/courses?x=1",
    "/c/s1/m/2",
    "/a//b",
    "/a\\b",
    "/.well-known/x",
    "/a/...",
    "/a/..b",
    "/a?x=/../y",
    "/a#/../b",
  ])("keeps %j", (value) => {
    expect(safeReturnTarget(value)).toBe(value);
  });

  it("refuses what is not a string", () => {
    expect(safeReturnTarget(undefined)).toBe("/");
    expect(safeReturnTarget(42)).toBe("/");
    expect(safeReturnTarget(["/x"])).toBe("/");
  });

  it("keeps a path of the server's maximum length and refuses a longer one", () => {
    const longest = `/${"a".repeat(MAX_RETURN_TARGET - 1)}`;
    expect(safeReturnTarget(longest)).toBe(longest);
    expect(safeReturnTarget(`${longest}a`)).toBe("/");
  });
});
