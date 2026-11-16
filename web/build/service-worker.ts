// SPDX-License-Identifier: AGPL-3.0-or-later
// The Vite plugin that builds the service worker as `dist/sw.js`
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md B3, Q1): `src/sw/worker.ts` is
// bundled as its own chunk, then the build's precache list (`/index.html` and every file under
// `assets/`, which Vite fingerprints) and the build's id are written in place of the worker's
// `__SC_PRECACHE__` and `__SC_BUILD__`. The id is a hash of the worker's code and the list, so a
// build that changes any shell file or the worker gets a new worker and a new cache.
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";

/** The worker's file in the build, which the server answers at `/sw.js`. */
export const WORKER_FILE = "sw.js";

const WORKER_SOURCE = fileURLToPath(new URL("../src/sw/worker.ts", import.meta.url));

/** `/index.html` and every fingerprinted file of the build, as paths, sorted. */
export function precacheList(files: readonly string[]): string[] {
  const assets = files
    .filter((file) => file.startsWith("assets/") && !file.endsWith(".map"))
    .map((file) => `/${file}`)
    .sort();
  return ["/index.html", ...assets];
}

/** The worker's code with its build id and precache list written in. */
export function injectShell(code: string, precache: readonly string[]): string {
  for (const name of ["__SC_BUILD__", "__SC_PRECACHE__"]) {
    if (!code.includes(name)) throw new Error(`the service worker no longer reads ${name}`);
  }
  const list = JSON.stringify(precache);
  const build = createHash("sha256").update(code).update(list).digest("hex").slice(0, 16);
  return code.replaceAll("__SC_BUILD__", JSON.stringify(build)).replaceAll("__SC_PRECACHE__", list);
}

export function serviceWorker(): Plugin {
  return {
    name: "stepclimb-service-worker",
    apply: "build",
    enforce: "post",
    buildStart() {
      this.emitFile({ type: "chunk", id: WORKER_SOURCE, fileName: WORKER_FILE });
    },
    generateBundle(_options, bundle) {
      const worker = bundle[WORKER_FILE];
      if (worker?.type !== "chunk") return this.error(`the build has no ${WORKER_FILE} chunk`);
      // A classic script: the worker is registered without `type: "module"`, which Firefox and
      // older Safari do not run.
      if (worker.imports.length > 0 || worker.dynamicImports.length > 0) {
        return this.error(`${WORKER_FILE} imports another chunk: ${worker.imports.join(", ")}`);
      }
      const files = Object.keys(bundle).filter((file) => file !== WORKER_FILE);
      worker.code = injectShell(worker.code, precacheList(files));
    },
  };
}
