// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
import { describe, expect, it } from "vitest";
import { injectShell, precacheList } from "./service-worker";

describe("the service worker build (spec 15 B3)", () => {
  it("precaches index.html and the fingerprinted files only", () => {
    expect(
      precacheList([
        "index.html",
        "theme.css",
        "icons/icon-192.png",
        "assets/index-b.js",
        "assets/index-b.js.map",
        "assets/font-a.woff2",
      ]),
    ).toEqual(["/index.html", "/assets/font-a.woff2", "/assets/index-b.js"]);
  });

  it("writes the build id and list in, and changes the id with either", () => {
    const code = "const b=__SC_BUILD__,p=__SC_PRECACHE__;";
    const one = injectShell(code, ["/index.html", "/assets/a.js"]);
    expect(one).toMatch(/^const b="[0-9a-f]{16}",p=\["\/index.html","\/assets\/a.js"\];$/);
    expect(injectShell(code, ["/index.html", "/assets/a.js"])).toBe(one);
    expect(injectShell(code, ["/index.html", "/assets/b.js"])).not.toBe(one);
    expect(injectShell(`${code} `, ["/index.html", "/assets/a.js"]).slice(0, 26)).not.toBe(
      one.slice(0, 26),
    );
  });

  it("refuses a worker that no longer reads its placeholders", () => {
    expect(() => injectShell("const p=__SC_PRECACHE__;", [])).toThrow(/__SC_BUILD__/);
  });
});
