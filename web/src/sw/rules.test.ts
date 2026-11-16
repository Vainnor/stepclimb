// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { route, staleShellCaches } from "./rules";

const origin = "https://lms.example.org";
const precache = new Set(["/index.html", "/assets/index-abc123.js"]);

const get = (path: string, mode = "cors") => ({ method: "GET", url: `${origin}${path}`, mode });

describe("the worker's routes (spec 15 B4)", () => {
  it("never answers the API, sign-in, OAuth, files, media or the docs, navigations included", () => {
    for (const path of [
      "/api/v1/me",
      "/api",
      "/api/openapi.json",
      "/auth/sign-out",
      "/auth/mock/callback?code=c&state=s",
      "/oauth/authorize?client_id=x",
      "/oauth/token",
      "/files/01900000/download",
      "/media/v/1.mp4",
      "/docs",
    ]) {
      for (const mode of ["cors", "navigate", "no-cors", "same-origin"]) {
        expect(route(get(path, mode), origin, precache), `${mode} ${path}`).toEqual({
          kind: "network",
        });
      }
    }
  });

  it("does not answer a precached path under a server prefix, whatever the list says", () => {
    const listed = new Set(["/api/v1/me", "/auth/sign-out"]);
    expect(route(get("/api/v1/me"), origin, listed)).toEqual({ kind: "network" });
    expect(route(get("/auth/sign-out"), origin, listed)).toEqual({ kind: "network" });
  });

  it("matches the server prefixes segment by segment", () => {
    expect(route(get("/apiary", "navigate"), origin, precache)).toEqual({ kind: "navigation" });
    expect(route(get("/authors", "navigate"), origin, precache)).toEqual({ kind: "navigation" });
  });

  it("answers only GETs from its own origin", () => {
    const post = { method: "POST", url: `${origin}/assets/index-abc123.js`, mode: "cors" };
    expect(route(post, origin, precache)).toEqual({ kind: "network" });
    const navigatePost = { method: "POST", url: `${origin}/sign-in`, mode: "navigate" };
    expect(route(navigatePost, origin, precache)).toEqual({ kind: "network" });
    const elsewhere = {
      method: "GET",
      url: "https://cdn.example.com/assets/index-abc123.js",
      mode: "cors",
    };
    expect(route(elsewhere, origin, precache)).toEqual({ kind: "network" });
  });

  it("sends a navigation to the network with the shell as its fallback", () => {
    expect(route(get("/", "navigate"), origin, precache)).toEqual({ kind: "navigation" });
    expect(route(get("/c/s1/m/2", "navigate"), origin, precache)).toEqual({ kind: "navigation" });
  });

  it("answers a precached asset from the cache, and nothing else", () => {
    expect(route(get("/assets/index-abc123.js"), origin, precache)).toEqual({
      kind: "precache",
      path: "/assets/index-abc123.js",
    });
    expect(route(get("/assets/index-abc123.js?x=1"), origin, precache)).toEqual({
      kind: "network",
    });
    expect(route(get("/assets/other.js"), origin, precache)).toEqual({ kind: "network" });
    expect(route(get("/theme.css?v=ab12"), origin, precache)).toEqual({ kind: "network" });
    expect(route(get("/manifest.webmanifest"), origin, precache)).toEqual({ kind: "network" });
    expect(route(get("/sw.js"), origin, precache)).toEqual({ kind: "network" });
  });

  it("answers an offline download's URL from that course's cache", () => {
    expect(route(get("/offline/c1/files/f1"), origin, precache)).toEqual({
      kind: "offline",
      cache: "stepclimb-offline-c1",
    });
    expect(route(get("/offline/c1/files/f1", "navigate"), origin, precache)).toEqual({
      kind: "offline",
      cache: "stepclimb-offline-c1",
    });
    expect(route(get("/offline/"), origin, precache)).toEqual({ kind: "offline", cache: null });
  });

  it("deletes only earlier shells at activate, never a download", () => {
    expect(
      staleShellCaches(
        ["stepclimb-shell-old", "stepclimb-shell-new", "stepclimb-offline-c1", "other"],
        "new",
      ),
    ).toEqual(["stepclimb-shell-old"]);
  });
});
