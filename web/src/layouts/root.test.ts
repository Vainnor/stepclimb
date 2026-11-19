// SPDX-License-Identifier: AGPL-3.0-or-later
// The address a Stepclimb UI link carries (docs/specs/m1/03-modules-and-items.md B17): only an
// absolute http or https URL passes unchanged; anything else is resolved as a route.
import { describe, expect, it } from "vitest";
import { linkHref } from "./root";

const build = (to: string) => `/built${to.startsWith("/") ? "" : "/"}${to}`;

describe("linkHref", () => {
  it("passes an absolute http or https URL through unchanged, in any letter case", () => {
    for (const to of ["https://host.example/x", "http://host.example/", "HTTPS://Host.example/a"]) {
      expect(linkHref(to, build)).toBe(to);
    }
  });

  it("resolves a path as a route", () => {
    expect(linkHref("/account", build)).toBe("/built/account");
  });

  it("never passes another scheme or a protocol-relative URL through unchanged", () => {
    for (const to of [
      "javascript:alert(1)",
      "JavaScript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "vbscript:msgbox(1)",
      "//evil.example",
      "//evil.example/https://host.example",
      "https:evil.example",
    ]) {
      expect(linkHref(to, build), to).not.toBe(to);
    }
  });
});
