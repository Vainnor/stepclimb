// SPDX-License-Identifier: AGPL-3.0-or-later
// The installable web app in Chromium against the built binary
// (docs/specs/m1/15-installable-web-app-and-offline-reading.md T1 to T3). Every other file runs
// with service workers blocked (playwright.config.ts); this one allows them. Each test has its
// own browser context, so its worker and caches start empty.
import { expect, type Page, test } from "./csp";
import { type StaticBuild, serveBuild } from "./static-build";

test.use({ serviceWorkers: "allow" });

interface Manifest {
  name: string;
  short_name: string;
  theme_color: string;
  background_color: string;
  icons: { src: string; sizes: string; type: string; purpose?: string }[];
}

/** Opens the app and waits until its worker is active. */
async function visit(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator("#root")).not.toBeEmpty();
  await page.evaluate(async () => {
    await navigator.serviceWorker.ready;
  });
}

/** Every cached URL, in every Cache Storage cache of the page's origin. */
async function cachedUrls(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const urls: string[] = [];
    for (const name of await caches.keys()) {
      const cache = await caches.open(name);
      for (const request of await cache.keys()) urls.push(request.url);
    }
    return urls;
  });
}

/** `color`, any CSS color, as the `#rrggbb` a canvas draws it with. */
async function drawn(page: Page, variable: string): Promise<string> {
  return page.evaluate((name) => {
    const probe = document.createElement("div");
    probe.dataset.scTheme = "light";
    document.body.append(probe);
    const color = getComputedStyle(probe).getPropertyValue(name).trim();
    probe.remove();
    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("no 2D canvas");
    context.fillStyle = color;
    context.fillRect(0, 0, 1, 1);
    const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
    return `#${[r, g, b].map((v) => (v ?? 0).toString(16).padStart(2, "0")).join("")}`;
  }, variable);
}

function channels(hex: string): number[] {
  return [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16));
}

test("the manifest names the organization in its colors, and Chromium finds it installable (T1)", async ({
  page,
}) => {
  await visit(page);
  const answer = await page.request.get("/manifest.webmanifest");
  expect(answer.status()).toBe(200);
  expect(answer.headers()["content-type"]).toBe("application/manifest+json");
  const manifest = (await answer.json()) as Manifest;
  expect(manifest.name).toBe("Example Training");
  expect(manifest.short_name).toBe("Example Trai");
  // The colors the browser draws the organization's light theme with, read off a canvas: the
  // manifest's are the same to within one level per channel.
  for (const [variable, value] of [
    ["--sc-brand", manifest.theme_color],
    ["--sc-bg-canvas", manifest.background_color],
  ] as const) {
    const shown = channels(await drawn(page, variable));
    const listed = channels(value);
    for (const [i, level] of shown.entries()) {
      expect(Math.abs(level - (listed[i] ?? -9)), variable).toBeLessThanOrEqual(1);
    }
  }
  await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
    "content",
    manifest.theme_color,
  );
  for (const icon of manifest.icons) {
    const file = await page.request.get(icon.src);
    expect(file.status(), icon.src).toBe(200);
    expect(file.headers()["content-type"], icon.src).toBe("image/png");
  }
  const cdp = await page.context().newCDPSession(page);
  const { errors } = (await cdp.send("Page.getAppManifest")) as { errors: unknown[] };
  expect(errors).toEqual([]);
  const { installabilityErrors } = (await cdp.send("Page.getInstallabilityErrors")) as {
    installabilityErrors: unknown[];
  };
  expect(installabilityErrors).toEqual([]);
});

test("offline, a reload opens the app shell, and no API answer comes from the worker (T2)", async ({
  page,
  context,
}) => {
  const first = page.waitForResponse(
    (response) => new URL(response.url()).pathname === "/api/v1/me",
  );
  await visit(page);
  expect((await first).fromServiceWorker()).toBe(false);
  const shell = await cachedUrls(page);
  expect(shell.some((url) => url.endsWith("/index.html"))).toBe(true);
  expect(shell.some((url) => /\/assets\/index-[^/]+\.js$/.test(url))).toBe(true);

  // Controlled by the worker and online, the app asks `/api/v1/me`, the server answers this
  // anonymous browser 401, and the app goes to sign in.
  await page.reload();
  expect(await page.evaluate(() => navigator.serviceWorker.controller !== null)).toBe(true);
  await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();

  // Offline, the worker answers the page and its files, and `/api/v1/me` fails: the app says the
  // server could not be reached. A cached 401 would have sent it to sign in instead.
  await context.setOffline(true);
  await page.reload();
  await expect(
    page.getByRole("heading", { level: 1, name: "The server could not be reached" }),
  ).toBeVisible();
  await context.setOffline(false);

  const cached = await cachedUrls(page);
  expect(cached.filter((url) => new URL(url).pathname.startsWith("/api/"))).toEqual([]);
  expect(cached.sort()).toEqual(shell.sort());
});

test.describe("a new build", () => {
  let build: StaticBuild;
  test.beforeEach(async () => {
    build = await serveBuild();
  });
  test.afterEach(async () => {
    await build.close();
  });

  test("shows the update toast, and Reload runs the new build (T3)", async ({ page }) => {
    await page.goto(`${build.url}/`);
    await page.evaluate(async () => {
      await navigator.serviceWorker.ready;
    });
    const before = await page.evaluate(() => caches.keys());
    expect(before).toHaveLength(1);
    // Deploy the next build: the same worker with another build id, so another shell cache.
    const next = "e2e0000000000001";
    build.worker = build.worker.replace(/"[0-9a-f]{16}"/, `"${next}"`);
    expect(build.worker).toContain(next);
    await page.reload();

    const toast = page.getByRole("alertdialog", { name: "A new version is available" });
    await expect(toast).toBeVisible();
    expect(await page.evaluate(() => caches.keys())).toContain(`stepclimb-shell-${next}`);
    const reloaded = page.waitForEvent("load");
    await toast.getByRole("button", { name: "Reload" }).click();
    await reloaded;
    await expect
      .poll(() => page.evaluate(() => caches.keys()))
      .toEqual([`stepclimb-shell-${next}`]);
    await expect(toast).toBeHidden();
    expect(await page.evaluate(() => navigator.serviceWorker.controller?.state)).toBe("activated");
  });
});
