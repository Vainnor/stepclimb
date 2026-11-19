// SPDX-License-Identifier: AGPL-3.0-or-later
// A file item's preview (docs/specs/m1/06-files.md B18, T15, Q8) in Chromium, Firefox and WebKit:
// a PDF item shows the preview frame where the browser shows PDFs in the page, or the message and
// a download that works where it does not; the download asks for an `attachment` URL; an image
// item shows the image. `/api/v1/me`, the item reads and the signed URL route are answered in the
// browser with synthetic data; the signed file itself is answered with the synthetic PDF and PNG
// of `fixtures/` under the exact headers the server sends for them
// (`fixtures/signed-file-headers.json`, which `tests/file_delivery.rs` checks against the server),
// so each browser is tested against what it would get. Each browser's outcome is attached to the
// test as `outcome` for the pull request. Playwright's Linux WebKit says it shows PDFs but draws
// an empty frame (spec 06 Q8, coordinator decision 2026-11-19): there the test requires the
// frame and the download only, and Safari and the iOS web view are checked by hand with spec 17.
// It also fires no download event for a routed answer, so its download is checked as the
// attachment URL fetched as a navigation that leaves the page where it was. In Chromium the item view is also checked with axe and
// saved as screenshots in the default and vZDC brands, light and dark, at 1280 and 375 px.
import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type Route, test } from "./csp";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

const fixture = (name: string) => readFileSync(new URL(`fixtures/${name}`, import.meta.url));
const PDF = fixture("handbook.pdf");
const PNG = fixture("diagram.png");
const HEADERS = JSON.parse(fixture("signed-file-headers.json").toString("utf8")) as Record<
  string,
  Record<string, string>
>;

const allBrands = readFileSync(
  new URL("../../packages/tokens/build/css/variables.css", import.meta.url),
  "utf8",
);

const courseId = "01900000-0000-7000-8000-0000000000c1";
const moduleId = "01900000-0000-7000-8000-0000000000d1";
const pdfItem = "01900000-0000-7000-8000-0000000000e1";
const imageItem = "01900000-0000-7000-8000-0000000000e2";
const link = (item: string) => `/c/${courseId}/m/${moduleId}/i/${item}`;

function meWith(theme: string) {
  return {
    user: {
      id: "01900000-0000-7000-8000-000000000002",
      display_name: "Avery Example",
      given_name: "Avery",
      family_name: "Example",
      email: "avery@example.test",
      locale: "en",
      tz: "UTC",
      status: "active",
      source: "local",
    },
    organization: {
      id: "01900000-0000-7000-8000-000000000001",
      slug: "example",
      name: "Example Training",
    },
    grants: [],
    features: [],
    appearance: { theme, density: "comfortable" },
    default_appearance: "system",
    embedding: { return_origins: [] },
  };
}

function item(id: string, title: string) {
  return {
    id,
    module_id: moduleId,
    course_id: courseId,
    kind: "file",
    title,
    position: 0,
    published: true,
    ref_id: "01900000-0000-7000-8000-0000000000f1",
    requirement: { kind: "view" },
    lock: null,
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
  };
}

/** The signed file each item's URL serves, by item id. */
const FILES: Record<string, { name: string; mime: string; bytes: Buffer; key: string }> = {
  [pdfItem]: {
    name: "Ground school handbook.pdf",
    mime: "application/pdf",
    bytes: PDF,
    key: "pdf",
  },
  [imageItem]: { name: "Airport diagram.png", mime: "image/png", bytes: PNG, key: "png" },
};

/** Answers the API and the signed files; returns the dispositions asked for, in order. */
async function serveItems(page: Page, theme: string, brand = "default"): Promise<string[]> {
  const asked: string[] = [];
  if (brand !== "default") {
    await page.route("**/theme.css*", (route) =>
      route.fulfill({ contentType: "text/css", body: allBrands }),
    );
  }
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route(`**/api/v1/items/${pdfItem}`, (route) =>
    route.fulfill({ json: item(pdfItem, "Ground school handbook") }),
  );
  await page.route(`**/api/v1/items/${imageItem}`, (route) =>
    route.fulfill({ json: item(imageItem, "Airport diagram") }),
  );
  await page.route("**/api/v1/items/*/file-url", async (route: Route) => {
    const id = new URL(route.request().url()).pathname.split("/")[4] ?? "";
    const file = FILES[id];
    if (file === undefined) return route.fulfill({ status: 404 });
    const { disposition } = route.request().postDataJSON() as { disposition: string };
    asked.push(disposition);
    const origin = new URL(page.url()).origin;
    return route.fulfill({
      json: {
        url: `${origin}/files/${file.key}-${disposition}`,
        expires_at: "2099-01-01T00:00:00.000000Z",
        disposition,
        name: file.name,
        mime: file.mime,
        size: file.bytes.length,
      },
    });
  });
  await page.route("**/files/*", (route: Route) => {
    const token = new URL(route.request().url()).pathname.split("/").pop() ?? "";
    const headers = HEADERS[token];
    const file = Object.values(FILES).find((f) => token.startsWith(`${f.key}-`));
    if (headers === undefined || file === undefined) return route.fulfill({ status: 404 });
    return route.fulfill({ status: 200, headers, body: file.bytes });
  });
  return asked;
}

async function drawBrand(page: Page, brand: string) {
  if (brand === "default") return;
  await page.evaluate((b) => {
    document.documentElement.dataset.scBrand = b;
  }, brand);
}

test("a PDF item previews or offers a working download, as the browser can (T15)", async ({
  page,
  browserName,
}, info) => {
  const asked = await serveItems(page, "light");
  await page.goto(link(pdfItem));
  await expect(
    page.getByRole("heading", { level: 2, name: "Ground school handbook" }),
  ).toBeVisible();
  const download = page.getByRole("button", { name: "Download" });
  await expect(download).toBeVisible();
  const showsPdf = await page.evaluate(() => navigator.pdfViewerEnabled === true);
  const frame = page.getByTitle("Ground school handbook (PDF preview)");
  const fallback = page.getByText(
    "This browser does not show PDF files in the page. Download the file to read it.",
  );
  if (showsPdf && browserName === "webkit") {
    await expect(frame).toBeVisible();
    await expect(fallback).toHaveCount(0);
  } else if (showsPdf) {
    await expect(frame).toBeVisible();
    await expect(fallback).toHaveCount(0);
    // The frame loaded the signed URL, and the browser kept it in the page: no download.
    await expect
      .poll(() => page.frames().some((f) => f.url().endsWith("/files/pdf-inline")))
      .toBe(true);
  } else {
    await expect(fallback).toBeVisible();
    await expect(frame).toHaveCount(0);
  }
  await page.screenshot({ path: info.outputPath(`${browserName}-pdf.png`), fullPage: true });

  const here = page.url();
  if (browserName === "webkit") {
    // Playwright's Linux WebKit fires no download event for the routed answer: the check is
    // that it asks for the attachment URL as a navigation, gets it, and stays on the item.
    const fetched = page.waitForResponse(
      (r) => r.url().endsWith("/files/pdf-attachment") && r.request().isNavigationRequest(),
    );
    await download.click();
    const answer = await fetched;
    expect(answer.status()).toBe(200);
    expect(answer.headers()["content-disposition"]).toMatch(/^attachment;/);
    await page.waitForTimeout(500);
    expect(page.url()).toBe(here);
  } else {
    const saved = page.waitForEvent("download");
    await download.click();
    const file = await saved;
    expect(file.suggestedFilename()).toBe("Ground school handbook.pdf");
    expect(file.url()).toMatch(/\/files\/pdf-attachment$/);
    expect(page.url()).toBe(here);
  }
  expect(asked.at(-1)).toBe("attachment");
  await info.attach("outcome", {
    body: `${browserName}: ${showsPdf ? "preview frame" : "fallback with download"}${
      showsPdf && browserName === "webkit" ? " (empty in Playwright's Linux WebKit)" : ""
    }; download works`,
    contentType: "text/plain",
  });
});

test("an image item shows the image (T15)", async ({ page }) => {
  await serveItems(page, "light");
  await page.goto(link(imageItem));
  const image = page.getByRole("img", { name: "Airport diagram" });
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(96);
});

const brands = ["default", "vzdc"] as const;
const themes = ["light", "dark"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

for (const brand of brands) {
  for (const theme of themes) {
    for (const viewport of widths) {
      test(`a file item has no axe violations, ${brand} ${theme} at ${viewport.width} px`, async ({
        page,
        browserName,
      }, info) => {
        test.skip(browserName !== "chromium", "the themes are drawn once, in Chromium");
        await page.setViewportSize(viewport);
        await serveItems(page, theme, brand);
        for (const [id, title, state] of [
          [pdfItem, "Ground school handbook", "pdf"],
          [imageItem, "Airport diagram", "image"],
        ] as const) {
          await page.goto(link(id));
          await drawBrand(page, brand);
          await expect(page.getByRole("heading", { level: 2, name: title })).toBeVisible();
          await expect(page.getByRole("button", { name: "Download" })).toBeVisible();
          expect(await violations(page)).toEqual([]);
          await page.screenshot({
            path: info.outputPath(`${brand}-${theme}-${viewport.width}-${state}.png`),
            fullPage: true,
          });
        }
      });
    }
  }
}
