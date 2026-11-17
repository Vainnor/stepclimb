// SPDX-License-Identifier: AGPL-3.0-or-later
// The files area's accessibility (docs/specs/m1/16-learning-ui-components.md B2, B20): the folder
// list with its tree, an upload the server refuses because storage is not set up, the row
// actions menu and the rename dialog, in light, dark and high contrast at 1280 and 375 px. As in
// account.spec.ts, `/api/v1/me`, the course and the file routes are answered in the browser with
// synthetic data; the rest is the built binary serving the built SPA.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** axe's WCAG 2.2 AA violations on the whole page, one line per node. */
async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

const themes = ["light", "dark", "high-contrast"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

const courseId = "01900000-0000-7000-8000-0000000000c1";

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
  };
}

const course = {
  id: courseId,
  account_id: "01900000-0000-7000-8000-0000000000a0",
  code: "GND-101",
  title: "Ground school",
  status: "draft",
  template: false,
  permissions: ["course.content_edit", "course.view", "course.view_all"],
  settings: { home: { kind: "modules" } },
  start_at: null,
  end_at: null,
  created_at: "2026-11-01T12:00:00Z",
  updated_at: "2026-11-01T12:00:00Z",
};

function file(name: string, size: number, mime: string) {
  return {
    kind: "file",
    file: {
      id: `01900000-0000-7000-8000-${String(size).padStart(12, "0")}`,
      course_id: courseId,
      folder: "/",
      name,
      size,
      mime,
      sha256: "0".repeat(64),
      created_at: "2026-11-02T12:00:00Z",
      updated_at: "2026-11-03T12:00:00Z",
    },
  };
}

const entries = [
  { kind: "folder", path: "/handouts", name: "handouts" },
  file("Ground school handbook.pdf", 2_400_000, "application/pdf"),
  file("Airport diagram.png", 380_000, "image/png"),
];

/** Answers `/me`, the course and its file routes in the browser; uploads find no storage. */
async function serveFiles(page: Page, theme: string) {
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route(`**/api/v1/courses/${courseId}`, (route) => route.fulfill({ json: course }));
  await page.route(`**/api/v1/courses/${courseId}/files?*`, (route) => {
    if (route.request().method() === "POST") {
      return route.fulfill({
        status: 503,
        contentType: "application/problem+json",
        body: JSON.stringify({
          type: "about:blank",
          title: "Service Unavailable",
          status: 503,
          detail: "Storage is not configured: set STEPCLIMB_STORAGE_URL.",
          code: "storage.not_configured",
        }),
      });
    }
    const folder = new URL(route.request().url()).searchParams.get("folder");
    return route.fulfill({ json: { items: folder === "/" ? entries : [], next_cursor: null } });
  });
}

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the files area has no axe violations, ${theme} at ${viewport.width} px`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await serveFiles(page, theme);
      await page.goto(`/c/${courseId}/files`);
      await expect(page.getByRole("heading", { level: 1, name: "Files" })).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      const list = page.getByRole("grid", { name: "Files in Top folder" });
      await expect(list.getByRole("row")).toHaveCount(4);
      // The tree is beside the list from 768 px; under it the breadcrumbs and folder rows stay.
      await expect(page.getByRole("treegrid", { name: "Folders" })).toBeVisible({
        visible: viewport.width >= 768,
      });
      expect(await violations(page)).toEqual([]);

      await page.locator('[data-slot="file-drop"] input[type="file"]').setInputFiles({
        name: "Notes.pdf",
        mimeType: "application/pdf",
        buffer: Buffer.from("%PDF-1.7 synthetic"),
      });
      await expect(page.getByText(/File storage is not set up on this server/)).toBeVisible();
      expect(await violations(page)).toEqual([]);

      // From the keyboard: under 768 px a mouse press opens the menu as a bottom sheet, and the
      // release of a press low on the page lands on one of its items.
      await page.getByRole("button", { name: "Actions for Airport diagram.png" }).focus();
      await page.keyboard.press("Enter");
      await page.getByRole("menuitem", { name: "Rename" }).click();
      const rename = page.getByRole("dialog", { name: "Rename Airport diagram.png" });
      await expect(rename.getByRole("textbox", { name: "Name" })).toBeFocused();
      expect(await violations(page)).toEqual([]);
    });
  }
}
