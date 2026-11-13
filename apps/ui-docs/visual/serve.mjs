// SPDX-License-Identifier: AGPL-3.0-or-later
// Serves the static Storybook build (storybook-static/) for the Playwright story suite. It runs
// inside the Playwright container, so it needs nothing beyond Node.
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../storybook-static/", import.meta.url));
const port = Number(process.env.SC_STORYBOOK_PORT ?? "6007");

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ico": "image/x-icon",
};

/** The file under `root` a request path names, or undefined when it escapes `root` or is absent. */
function fileFor(urlPath) {
  const path = normalize(decodeURIComponent(urlPath.split("?")[0] ?? "/"));
  const file = join(root, path.endsWith("/") ? `${path}index.html` : path);
  if (!file.startsWith(root)) return undefined;
  try {
    return statSync(file).isFile() ? file : undefined;
  } catch (err) {
    if (err.code === "ENOENT" || err.code === "ENOTDIR") return undefined;
    throw err;
  }
}

createServer((req, res) => {
  const file = fileFor(req.url ?? "/");
  if (!file) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
  createReadStream(file).pipe(res);
}).listen(port, "127.0.0.1", () => {
  console.log(`storybook-static on http://127.0.0.1:${port}`);
});
