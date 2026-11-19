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
  completion_valid_months: null,
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

/**
 * The text of the row holding focus once the list has settled for half a second, or `body` when
 * focus fell to the page. Focus that lands and is then lost does not pass.
 */
async function focusedRow(page: Page): Promise<string> {
  await page.waitForTimeout(500);
  return page.evaluate(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) return "body";
    return active.closest('[role="row"]')?.textContent ?? active.tagName;
  });
}

/** A folder entry as the routes here answer it: a folder, or a file. */
interface Entry {
  kind: string;
  path?: string;
  name?: string;
  file?: { id: string; name: string };
}

/** A folder whose entries change as the routes answer, as the server's would. */
async function serveChangingFolder(page: Page) {
  let items: Entry[] = [
    { kind: "folder", path: "/charts", name: "charts" },
    { kind: "folder", path: "/handouts", name: "handouts" },
    file("Airport diagram.png", 380_000, "image/png"),
    file("Briefing.pdf", 2_400_000, "application/pdf"),
    file("Checklist.txt", 812, "text/plain"),
  ];
  const named = (name: string) =>
    items.find((entry) => entry.kind === "file" && entry.file?.name === name);
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith("light") }));
  await page.route(`**/api/v1/courses/${courseId}`, (route) => route.fulfill({ json: course }));
  await page.route(`**/api/v1/courses/${courseId}/files?*`, (route) => {
    const folder = new URL(route.request().url()).searchParams.get("folder");
    return route.fulfill({ json: { items: folder === "/" ? items : [], next_cursor: null } });
  });
  await page.route("**/api/v1/course-files/*", (route) => {
    const id = new URL(route.request().url()).pathname.split("/").at(-1);
    const entry = items.find((e) => e.kind === "file" && e.file?.id === id);
    items = items.filter((e) => e !== entry);
    if (route.request().method() === "DELETE") return route.fulfill({ status: 204 });
    return route.fulfill({ json: { ...entry?.file, folder: "/handouts" } });
  });
  await page.route(`**/api/v1/courses/${courseId}/folders/rename`, (route) => {
    items = items.map((e) =>
      e.kind === "folder" && e.path === "/charts" ? { ...e, path: "/maps", name: "maps" } : e,
    );
    // Sub-folders first, each by name, as the listing orders them.
    const nameOf = (e: Entry) => (e.kind === "folder" ? (e.name ?? "") : "");
    items.sort(
      (a, b) =>
        Number(a.kind === "file") - Number(b.kind === "file") || nameOf(a).localeCompare(nameOf(b)),
    );
    return route.fulfill({ json: { moved: 2 } });
  });
  return { named };
}

/** Opens a row's actions menu from the keyboard and picks `action`. */
async function rowAction(page: Page, name: string, action: string) {
  await page.getByRole("button", { name: `Actions for ${name}` }).focus();
  await page.keyboard.press("Enter");
  await page.getByRole("menuitem", { name: action }).click();
}

for (const viewport of widths) {
  test(`focus stays in the list after each row dialog closes at ${viewport.width} px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    const { named } = await serveChangingFolder(page);
    await page.goto(`/c/${courseId}/files`);
    const list = page.getByRole("grid", { name: "Files in Top folder" });
    await expect(list.getByRole("row")).toHaveCount(6);

    // Cancel: back on the row's actions button.
    await rowAction(page, "Briefing.pdf", "Rename");
    await page.getByRole("dialog").getByRole("button", { name: "Cancel" }).click();
    await expect(page.getByRole("button", { name: "Actions for Briefing.pdf" })).toBeFocused();

    // Delete: the row now in its place.
    await rowAction(page, "Briefing.pdf", "Delete");
    await page.getByRole("alertdialog").getByRole("button", { name: "Delete file" }).click();
    await expect(list.getByRole("row")).toHaveCount(5);
    expect(await focusedRow(page)).toContain("Checklist.txt");

    // Move out: the row now in its place.
    await rowAction(page, "Airport diagram.png", "Move");
    const move = page.getByRole("dialog", { name: "Move Airport diagram.png" });
    await move.getByRole("textbox", { name: "Folder" }).fill("/handouts");
    await move.getByRole("button", { name: "Move file" }).click();
    await expect(list.getByRole("row")).toHaveCount(4);
    expect(named("Airport diagram.png")).toBeUndefined();
    expect(await focusedRow(page)).toContain("Checklist.txt");

    // Folder rename: the row now in its place.
    await rowAction(page, "charts", "Rename");
    const rename = page.getByRole("dialog", { name: "Rename folder charts" });
    await rename.getByRole("textbox", { name: "Name" }).fill("maps");
    await rename.getByRole("button", { name: "Save name" }).click();
    await expect(list.getByRole("link", { name: "maps" })).toBeVisible();
    expect(await focusedRow(page)).toContain("handouts");
  });
}
