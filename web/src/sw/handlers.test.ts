// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
// The worker's steps against in-memory caches and a recorded network: what install stores, what
// activate deletes, and that the API never reaches a cache (spec 15 B3, B4, T2).
import { describe, expect, it } from "vitest";
import { activate, install, respond, type Shell } from "./handlers";

const origin = "https://lms.example.org";

class MemoryCache {
  readonly entries = new Map<string, Response>();
  async match(url: string) {
    return this.entries.get(url)?.clone();
  }
  async put(url: string, response: Response) {
    this.entries.set(url, response);
  }
}

class MemoryCaches {
  readonly stores = new Map<string, MemoryCache>();
  async open(name: string) {
    let cache = this.stores.get(name);
    if (!cache) {
      cache = new MemoryCache();
      this.stores.set(name, cache);
    }
    return cache;
  }
  async has(name: string) {
    return this.stores.has(name);
  }
  async delete(name: string) {
    return this.stores.delete(name);
  }
  async keys() {
    return [...this.stores.keys()];
  }
  /** Every URL stored in any cache. */
  urls(): string[] {
    return [...this.stores.values()].flatMap((cache) => [...cache.entries.keys()]);
  }
}

interface Network {
  requests: Request[];
  offline: boolean;
  status: number;
}

function world(build = "b1", precache = ["/index.html", "/assets/index-abc.js"]) {
  const caches = new MemoryCaches();
  const network: Network = { requests: [], offline: false, status: 200 };
  const shell: Shell = {
    build,
    precache,
    origin,
    caches: caches as unknown as CacheStorage,
    fetch: async (request) => {
      network.requests.push(request);
      if (network.offline) throw new TypeError("Failed to fetch");
      const path = new URL(request.url).pathname;
      return new Response(`network ${path}`, { status: network.status });
    },
  };
  return { caches, network, shell, precached: new Set(precache) };
}

/** A navigation request; `Request` refuses `mode: "navigate"` outside a browser. */
function navigation(path: string): Request {
  const request = new Request(`${origin}${path}`);
  Object.defineProperty(request, "mode", { value: "navigate" });
  return request;
}

describe("the worker's steps (spec 15 B3, B4)", () => {
  it("installs the shell past the HTTP cache into the build's cache", async () => {
    const { caches, network, shell } = world();
    await install(shell);
    expect(await caches.keys()).toEqual(["stepclimb-shell-b1"]);
    expect(caches.urls().sort()).toEqual([`${origin}/assets/index-abc.js`, `${origin}/index.html`]);
    expect(network.requests.map((r) => r.cache)).toEqual(["reload", "reload"]);
  });

  it("fails an install with a missing file and keeps nothing of it", async () => {
    const { caches, network, shell } = world();
    network.status = 404;
    await expect(install(shell)).rejects.toThrow(/precaching/);
    expect(await caches.keys()).toEqual([]);
  });

  it("deletes the shells of earlier builds at activate, and keeps downloads", async () => {
    const { caches, shell } = world("b2");
    await caches.open("stepclimb-shell-b1");
    await caches.open("stepclimb-offline-c1");
    await install(shell);
    await activate(shell);
    expect((await caches.keys()).sort()).toEqual(["stepclimb-offline-c1", "stepclimb-shell-b2"]);
  });

  it("answers a navigation from the network, and from the cached shell offline", async () => {
    const { network, shell, precached } = world();
    await install(shell);
    const online = await respond(navigation("/c/s1"), shell, precached);
    expect(await online?.text()).toBe("network /c/s1");
    network.offline = true;
    const offline = await respond(navigation("/c/s1"), shell, precached);
    expect(await offline?.text()).toBe("network /index.html");
  });

  it("answers a precached asset from the cache, without the network", async () => {
    const { network, shell, precached } = world();
    await install(shell);
    network.requests = [];
    const answer = await respond(new Request(`${origin}/assets/index-abc.js`), shell, precached);
    expect(await answer?.text()).toBe("network /assets/index-abc.js");
    expect(network.requests).toEqual([]);
  });

  it("leaves every API, sign-in and file request to the network, and caches none (T2)", async () => {
    const { caches, network, shell, precached } = world();
    await install(shell);
    const before = caches.urls().sort();
    network.requests = [];
    for (const path of ["/api/v1/me", "/auth/sign-out", "/oauth/token", "/files/f1", "/media/m1"]) {
      expect(respond(new Request(`${origin}${path}`), shell, precached), path).toBeUndefined();
      expect(respond(navigation(path), shell, precached), path).toBeUndefined();
    }
    expect(network.requests).toEqual([]);
    expect(caches.urls().sort()).toEqual(before);
    expect(caches.urls().some((url) => new URL(url).pathname.startsWith("/api/"))).toBe(false);
  });

  it("answers an offline download's URL from its course's cache, or 404", async () => {
    const { caches, network, shell, precached } = world();
    const url = `${origin}/offline/c1/files/f1`;
    const missing = await respond(new Request(url), shell, precached);
    expect(missing?.status).toBe(404);
    // A miss never creates the course's cache.
    expect(await caches.has("stepclimb-offline-c1")).toBe(false);
    const course = await caches.open("stepclimb-offline-c1");
    await course.put(url, new Response("the file"));
    const hit = await respond(new Request(url), shell, precached);
    expect(await hit?.text()).toBe("the file");
    const other = await respond(new Request(`${origin}/offline/c1/files/f2`), shell, precached);
    expect(other?.status).toBe(404);
    expect(network.requests).toEqual([]);
  });
});
