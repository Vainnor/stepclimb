// SPDX-License-Identifier: AGPL-3.0-or-later
// The service worker, built as `/sw.js` (docs/specs/m1/15-installable-web-app-and-offline-reading.md
// B3 to B5; `build/service-worker.ts` bundles it and writes the build's id and precache list in
// place of the two constants below). A new build's worker installs and then waits; the page's
// "Reload" sends it `skip-waiting`, and it takes over.
import { activate, install, respond, type Shell } from "./handlers";

declare const __SC_BUILD__: string;
declare const __SC_PRECACHE__: readonly string[];

/** The parts of the worker's global scope this file uses (`lib.webworker` is not in the build). */
interface ExtendableEvent extends Event {
  waitUntil(promise: Promise<unknown>): void;
}
interface FetchEvent extends ExtendableEvent {
  request: Request;
  respondWith(response: Promise<Response>): void;
}
interface MessageEventLike extends ExtendableEvent {
  data: unknown;
}
interface WorkerScope {
  location: Location;
  skipWaiting(): Promise<void>;
  addEventListener(type: "install" | "activate", listener: (event: ExtendableEvent) => void): void;
  addEventListener(type: "fetch", listener: (event: FetchEvent) => void): void;
  addEventListener(type: "message", listener: (event: MessageEventLike) => void): void;
}

const scope = self as unknown as WorkerScope;

const shell: Shell = {
  build: __SC_BUILD__,
  precache: __SC_PRECACHE__,
  origin: scope.location.origin,
  caches,
  fetch: (request) => fetch(request),
};
const precached = new Set(shell.precache);

scope.addEventListener("install", (event) => event.waitUntil(install(shell)));
scope.addEventListener("activate", (event) => event.waitUntil(activate(shell)));
scope.addEventListener("fetch", (event) => {
  const answer = respond(event.request, shell, precached);
  if (answer) event.respondWith(answer);
});
scope.addEventListener("message", (event) => {
  const data = event.data;
  if (typeof data === "object" && data !== null && "type" in data && data.type === "skip-waiting") {
    event.waitUntil(scope.skipWaiting());
  }
});
