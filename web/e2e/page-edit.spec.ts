// SPDX-License-Identifier: AGPL-3.0-or-later
// The page authoring screen in the browser (docs/specs/m1/07-pages-and-the-editor.md B4, B21,
// T13, editor part): an author writes a page with a callout and an image and saves it, and the
// screen has no axe violation in light, dark and high contrast at 1280 and 375 px. As in
// account.spec.ts, the API is answered in the browser with synthetic data; the reader's side of
// T13 comes with PageReader (I4).
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type Request, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

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

const COURSE = "01900000-0000-7000-8000-0000000000c0";
const PAGE = "01900000-0000-7000-8000-0000000000c1";
const FILE = "01900000-0000-7000-8000-0000000000f1";
const AUTHOR = "01900000-0000-7000-8000-000000000002";
const PATH = `/c/${COURSE}/pages/${PAGE}/edit`;
const version = (n: number) => ({
  id: `01900000-0000-7000-8000-0000000000d${n}`,
  number: n,
  author_id: AUTHOR,
  created_at: "2026-11-17T12:00:00Z",
});
const body = {
  type: "doc",
  content: [
    { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "Before you taxi" }] },
    { type: "paragraph" },
  ],
};

function meWith(theme: string) {
  return {
    user: {
      id: AUTHOR,
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

/** Answers `/me`, the page's versions, the save and the image upload; returns the saves. */
async function servePage(page: Page, theme = "light"): Promise<Request[]> {
  const saves: Request[] = [];
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route(`**/api/v1/pages/${PAGE}/versions?*`, (route) =>
    route.fulfill({ json: { items: [version(1)], next_cursor: null } }),
  );
  await page.route(`**/api/v1/pages/${PAGE}/versions/${version(1).id}`, (route) =>
    route.fulfill({ json: { ...version(1), body } }),
  );
  await page.route(`**/api/v1/courses/${COURSE}/files?*`, (route) =>
    route.fulfill({ status: 201, json: { id: FILE, mime: "image/png", course_id: COURSE } }),
  );
  await page.route(`**/api/v1/pages/${PAGE}/body`, (route) => {
    saves.push(route.request());
    return route.fulfill({
      json: {
        id: PAGE,
        course_id: COURSE,
        title: "Taxi",
        slug: "taxi",
        current_version: version(2),
        created_at: "2026-11-17T12:00:00Z",
        updated_at: "2026-11-17T12:00:00Z",
      },
    });
  });
  return saves;
}

const editor = (page: Page) => page.getByRole("textbox", { name: "Page text" });

test("an author writes a page with a callout and an image and saves it", async ({ page }) => {
  const saves = await servePage(page);
  await page.goto(PATH);
  await expect(editor(page)).toContainText("Before you taxi");
  // The caret into the empty paragraph after the heading, then the slash menu from the keyboard.
  await editor(page).click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("/");
  const menu = page.getByRole("menu", { name: "Insert" });
  await expect(menu).toBeVisible();
  await page.keyboard.type("Callout");
  await page.keyboard.press("Enter");
  await expect(menu).toBeHidden();
  await page.keyboard.type("Hold short of every runway.");

  await page.getByRole("button", { name: "Insert" }).click();
  await page.getByRole("menuitem", { name: "Image" }).click();
  const panel = page.getByRole("form", { name: "Insert an image" });
  await panel.locator("input[type=file]").setInputFiles({
    name: "diagram.png",
    mimeType: "image/png",
    buffer: Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  });
  await panel.getByRole("textbox", { name: "Text alternative" }).fill("The taxiway diagram");
  await expect(panel.getByRole("button", { name: "Insert image" })).toBeEnabled();
  await panel.getByRole("button", { name: "Insert image" }).click();
  await expect(editor(page)).toContainText("Image: The taxiway diagram");

  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved as version 2").first()).toBeVisible();
  expect(saves).toHaveLength(1);
  const sent = saves[0]?.postDataJSON();
  expect(sent.base_version_id).toBe(version(1).id);
  expect(sent.body.content).toContainEqual({
    type: "callout",
    attrs: { kind: "info" },
    content: [
      { type: "paragraph", content: [{ type: "text", text: "Hold short of every runway." }] },
    ],
  });
  expect(sent.body.content).toContainEqual({
    type: "image",
    attrs: { course_file_id: FILE, alt: "The taxiway diagram", decorative: false },
  });
});

test("Tab moves between table cells; Escape, then Tab, leaves the page text", async ({ page }) => {
  await servePage(page);
  const cell = (text: string) => ({
    type: "tableCell",
    attrs: { colspan: 1, rowspan: 1, colwidth: null },
    content: [{ type: "paragraph", content: [{ type: "text", text }] }],
  });
  await page.route(`**/api/v1/pages/${PAGE}/versions/${version(1).id}`, (route) =>
    route.fulfill({
      json: {
        ...version(1),
        body: {
          type: "doc",
          content: [{ type: "table", content: [{ type: "tableRow", content: [cell("a"), cell("b")] }] }],
        },
      },
    }),
  );
  await page.goto(PATH);
  await editor(page).getByText("a", { exact: true }).click();
  await page.keyboard.press("Tab");
  await expect(editor(page)).toBeFocused();
  await expect(page.getByRole("button", { name: "Add row" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Tab");
  await expect(editor(page)).not.toBeFocused();
});

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the page editor has no axe violations, ${theme} at ${viewport.width} px`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await servePage(page, theme);
      await page.goto(PATH);
      await expect(editor(page)).toContainText("Before you taxi");
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      expect(await violations(page)).toEqual([]);
      // From the keyboard: under 768 px the menu is a bottom sheet, and a mouse press opens it on
      // press start, so the release would land on whichever item the sheet drew under the pointer.
      await page.getByRole("button", { name: "Insert" }).press("Enter");
      await expect(page.getByRole("menuitem", { name: "Callout" })).toBeVisible();
      expect(await violations(page)).toEqual([]);
    });
  }
}
