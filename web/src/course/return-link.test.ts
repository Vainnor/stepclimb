// SPDX-License-Identifier: AGPL-3.0-or-later
// The return link's check (docs/specs/m1/03-modules-and-items.md B17, T11): only an absolute
// `https` URL on a listed origin makes a link, and the link goes where its label says.
import { describe, expect, it } from "vitest";
import { MAX_RETURN_URL, returnLink } from "./return-link";

const ORIGINS = ["https://host.example", "https://lms.example:8443"];

describe("returnLink", () => {
  it("links to a URL on a listed origin, named by its host (T11)", () => {
    expect(returnLink("https://host.example/x", ORIGINS)).toEqual({
      href: "https://host.example/x",
      host: "host.example",
    });
    expect(returnLink("https://lms.example:8443/a?b=1#c", ORIGINS)).toEqual({
      href: "https://lms.example:8443/a?b=1#c",
      host: "lms.example:8443",
    });
  });

  it("reads the origin as the browser does: letter case and the default port", () => {
    expect(returnLink("HTTPS://HOST.example:443/x", ORIGINS)).toEqual({
      href: "https://host.example/x",
      host: "host.example",
    });
  });

  it("makes no link for an origin the organization does not list (T11)", () => {
    for (const value of [
      "https://other.example/",
      "https://host.example.other.example/",
      "https://lms.example/",
      "https://lms.example:9443/",
      "https://sub.host.example/",
    ]) {
      expect(returnLink(value, ORIGINS), value).toBeUndefined();
    }
    expect(returnLink("https://host.example/x", [])).toBeUndefined();
  });

  it("refuses every other scheme and every form that is not an absolute https URL", () => {
    for (const value of [
      "http://host.example/x",
      "//host.example/x",
      "/x",
      "host.example/x",
      "https:host.example/x",
      "https:/host.example/x",
      "javascript:alert(1)//https://host.example",
      "data:text/html,https://host.example",
      " https://host.example/x",
    ]) {
      expect(returnLink(value, ORIGINS), value).toBeUndefined();
    }
  });

  it("refuses a user name or password, which can make an address read as another host", () => {
    expect(returnLink("https://host.example@other.example/", ORIGINS)).toBeUndefined();
    expect(returnLink("https://user:pw@host.example/", ORIGINS)).toBeUndefined();
  });

  it("follows the parser for a backslash, so the label and the link agree", () => {
    // A special scheme reads `\` as `/`: the host is other.example, which is not listed.
    expect(returnLink("https://other.example\\@host.example/", ORIGINS)).toBeUndefined();
    // Here the host is host.example and the rest is its path: the link stays on that origin.
    expect(returnLink("https://host.example\\@other.example/", ORIGINS)).toEqual({
      href: "https://host.example/@other.example/",
      host: "host.example",
    });
  });

  it("refuses white space, control and format characters instead of cleaning them", () => {
    for (const value of [
      "https://host.example/x\ty",
      "https://host.example/x\ny",
      "https://ho\nst.example/",
      "https://host.example/‮gnp.exe",
      "https://host.example/​",
      "https://host.example/ ",
      "https://host.example/\u0000",
    ]) {
      expect(returnLink(value, ORIGINS), JSON.stringify(value)).toBeUndefined();
    }
  });

  it("refuses a value that is not a string or is longer than 2,048 characters", () => {
    for (const value of [undefined, null, 1, ["https://host.example/"], {}]) {
      expect(returnLink(value, ORIGINS)).toBeUndefined();
    }
    const base = "https://host.example/";
    const longest = base + "a".repeat(MAX_RETURN_URL - base.length);
    expect(returnLink(longest, ORIGINS)?.host).toBe("host.example");
    expect(returnLink(`${longest}a`, ORIGINS)).toBeUndefined();
  });

  it("names a non-ASCII host by its ASCII form, the form the setting stores", () => {
    const link = returnLink("https://bücher.example/x", ["https://xn--bcher-kva.example"]);
    expect(link).toEqual({
      href: "https://xn--bcher-kva.example/x",
      host: "xn--bcher-kva.example",
    });
  });
});
