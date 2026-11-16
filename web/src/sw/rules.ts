// SPDX-License-Identifier: AGPL-3.0-or-later
// Which requests the service worker answers, and how
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md B4). Everything the worker does not
// answer goes to the network untouched: the worker never calls `respondWith` for it, so no API
// answer, credential or file reaches its caches.

/** The cache of one build's app shell is this prefix and the build's id. */
export const SHELL_CACHE_PREFIX = "stepclimb-shell-";

/** The cache of one course's offline download is this prefix and the course's id (B8). */
export const OFFLINE_CACHE_PREFIX = "stepclimb-offline-";

/** The cached page a navigation falls back to when the network fails. */
export const SHELL_PAGE = "/index.html";

/**
 * Paths the server answers itself: its API, sign-in, the OAuth 2.1 server, files, media and the
 * API docs. The worker never answers them, navigations included.
 */
export const NETWORK_ONLY = ["/api", "/auth", "/oauth", "/files", "/media", "/docs"] as const;

/** The synthetic URLs of offline downloads: `/offline/<course_id>/...`. */
const OFFLINE_PREFIX = "/offline/";

export type Route =
  /** Not answered by the worker. */
  | { kind: "network" }
  /** The network, then the cached app shell when the network fails. */
  | { kind: "navigation" }
  /** The precache of the current build. */
  | { kind: "precache"; path: string }
  /** An offline download's cache, or 404; `cache` is `null` for a URL that names no course. */
  | { kind: "offline"; cache: string | null };

export interface RequestLike {
  method: string;
  url: string;
  mode: string;
}

/** `path` is `prefix` or below it, segment by segment: `/api` and `/api/x`, never `/apiary`. */
function under(path: string, prefix: string): boolean {
  return path === prefix || path.startsWith(`${prefix}/`);
}

/**
 * How the worker of the app at `origin` handles `request`, given the paths it precached. Only
 * same-origin GETs are ever answered, and nothing under {@link NETWORK_ONLY}.
 */
export function route(request: RequestLike, origin: string, precache: ReadonlySet<string>): Route {
  if (request.method !== "GET") return { kind: "network" };
  const url = new URL(request.url);
  if (url.origin !== origin) return { kind: "network" };
  const path = url.pathname;
  if (NETWORK_ONLY.some((prefix) => under(path, prefix))) return { kind: "network" };
  if (path.startsWith(OFFLINE_PREFIX)) {
    const course = path.slice(OFFLINE_PREFIX.length).split("/")[0];
    return { kind: "offline", cache: course ? `${OFFLINE_CACHE_PREFIX}${course}` : null };
  }
  if (request.mode === "navigate") return { kind: "navigation" };
  if (url.search === "" && precache.has(path)) return { kind: "precache", path };
  return { kind: "network" };
}

/** The shell cache of the build `build`. */
export function shellCache(build: string): string {
  return `${SHELL_CACHE_PREFIX}${build}`;
}

/** The caches of earlier builds' shells, which activating `build` deletes. Downloads are kept. */
export function staleShellCaches(names: readonly string[], build: string): string[] {
  const current = shellCache(build);
  return names.filter((name) => name.startsWith(SHELL_CACHE_PREFIX) && name !== current);
}
