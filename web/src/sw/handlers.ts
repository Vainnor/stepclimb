// SPDX-License-Identifier: AGPL-3.0-or-later
// The service worker's install, activate and fetch steps
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md B3, B4), apart from the worker's
// global scope so tests can give them their own caches and network.
import { route, SHELL_PAGE, shellCache, staleShellCaches } from "./rules";

export interface Shell {
  /** The build's id, which names its cache. */
  build: string;
  /** `/index.html` and every hashed asset of the build, as paths. */
  precache: readonly string[];
  /** The origin the worker serves. */
  origin: string;
  caches: CacheStorage;
  fetch: (request: Request) => Promise<Response>;
}

/**
 * Caches the build's shell. Each file is fetched past the HTTP cache, so the shell is this
 * build's, and without cookies, so the cached `index.html` is the anonymous page the server
 * writes, carrying no user's appearance. A file that fails, or that answers with a redirect or
 * anything but a same-origin answer, fails the install: what it stored is removed, and the worker
 * of the build before stays.
 */
export async function install(shell: Shell): Promise<void> {
  const name = shellCache(shell.build);
  const cache = await shell.caches.open(name);
  try {
    await Promise.all(
      shell.precache.map(async (path) => {
        const url = new URL(path, shell.origin).href;
        const request = new Request(url, { cache: "reload", credentials: "omit" });
        const response = await shell.fetch(request);
        if (!response.ok) throw new Error(`precaching ${path}: ${response.status}`);
        if (response.redirected || response.type !== "basic") {
          throw new Error(
            `precaching ${path}: a ${response.type} answer, redirected: ${response.redirected}`,
          );
        }
        await cache.put(url, response);
      }),
    );
  } catch (error) {
    await shell.caches.delete(name);
    throw error;
  }
}

/** Deletes the shells of earlier builds. */
export async function activate(shell: Shell): Promise<void> {
  const stale = staleShellCaches(await shell.caches.keys(), shell.build);
  await Promise.all(stale.map((name) => shell.caches.delete(name)));
}

const notFound = () => new Response(null, { status: 404, statusText: "Not Found" });

/**
 * The answer to `request`, or `undefined` when the worker leaves it to the network untouched.
 * `precached` is the set of {@link Shell.precache}.
 */
export function respond(
  request: Request,
  shell: Shell,
  precached: ReadonlySet<string>,
): Promise<Response> | undefined {
  const decision = route(request, shell.origin, precached);
  switch (decision.kind) {
    case "network":
      return undefined;
    case "navigation":
      return shell.fetch(request).catch(async () => {
        const cache = await shell.caches.open(shellCache(shell.build));
        const page = await cache.match(new URL(SHELL_PAGE, shell.origin).href);
        return page ?? Response.error();
      });
    case "precache":
      return (async () => {
        const cache = await shell.caches.open(shellCache(shell.build));
        const hit = await cache.match(new URL(decision.path, shell.origin).href);
        return hit ?? shell.fetch(request);
      })();
    case "offline":
      return (async () => {
        if (decision.cache === null || !(await shell.caches.has(decision.cache))) {
          return notFound();
        }
        const cache = await shell.caches.open(decision.cache);
        const url = new URL(request.url);
        const hit = await cache.match(new URL(url.pathname, shell.origin).href);
        return hit ?? notFound();
      })();
  }
}
