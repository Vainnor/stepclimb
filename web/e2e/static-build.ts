// SPDX-License-Identifier: AGPL-3.0-or-later
// The built web app (`web/dist`) on a local HTTP server of the test's own, whose `/sw.js` a test
// can replace, as deploying a new build would. Spec 15 T3 needs this: the binary embeds its
// build, and Chromium fetches a worker's script out of `route`'s reach. A path that names no file
// gets `index.html`, as the binary answers a client route; every other request, `/api/` included,
// gets 404.
import { existsSync, readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));

const TYPES: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".png": "image/png",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

export interface StaticBuild {
  /** `http://127.0.0.1:<port>`, a secure context like localhost. */
  url: string;
  /** The worker the server answers at `/sw.js`, the build's to begin with. */
  worker: string;
  close(): Promise<void>;
}

export async function serveBuild(): Promise<StaticBuild> {
  if (!existsSync(join(DIST, "sw.js"))) {
    throw new Error("web/dist has no build: run `just web-build`");
  }
  const build = { worker: readFileSync(join(DIST, "sw.js"), "utf8") };
  const server: Server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://localhost").pathname;
    if (path === "/sw.js") {
      response.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-cache" });
      response.end(build.worker);
      return;
    }
    const file = normalize(join(DIST, path));
    const named = extname(path) !== "";
    if (named && file.startsWith(DIST) && existsSync(file) && !path.startsWith("/api/")) {
      response.writeHead(200, {
        "content-type": TYPES[extname(path)] ?? "application/octet-stream",
      });
      response.end(readFileSync(file));
      return;
    }
    if (named || path.startsWith("/api/") || path.startsWith("/auth/")) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { "content-type": "text/html", "cache-control": "no-cache" });
    response.end(readFileSync(join(DIST, "index.html")));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address() as AddressInfo;
  return Object.assign(build, {
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  });
}
