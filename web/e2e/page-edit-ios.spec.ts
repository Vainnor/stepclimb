// SPDX-License-Identifier: AGPL-3.0-or-later
// The page editor on a phone in WebKit, the engine of iOS Safari and the Tauri iOS web view
// (.claude/rules/cross-platform.md; docs/specs/m1/07-pages-and-the-editor.md B21): the text being
// written and every field are at least 16 px, so focusing them does not zoom the page, and Save
// sits in the editor's sticky bar, in view at the end of a long page. WebKit here has
// no software keyboard; the iPhone test (spec 17) checks that part on a device.
import { expect, type Page, test } from "@playwright/test";

const COURSE = "01900000-0000-7000-8000-0000000000c0";
const PAGE = "01900000-0000-7000-8000-0000000000c1";
const version = {
  id: "01900000-0000-7000-8000-0000000000d1",
  number: 1,
  author_id: "01900000-0000-7000-8000-000000000002",
  created_at: "2026-11-17T12:00:00Z",
};
const paragraphs = Array.from({ length: 30 }, (_, i) => ({
  type: "paragraph",
  content: [{ type: "text", text: `Paragraph ${i + 1} of a long page about taxi clearances.` }],
}));

async function servePage(page: Page) {
  await page.route("**/api/v1/me", (route) =>
    route.fulfill({
      json: {
        user: {
          id: version.author_id,
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
        appearance: { theme: "light", density: "comfortable" },
        default_appearance: "system",
      },
    }),
  );
  await page.route(`**/api/v1/pages/${PAGE}/versions?*`, (route) =>
    route.fulfill({ json: { items: [version], next_cursor: null } }),
  );
  await page.route(`**/api/v1/pages/${PAGE}/versions/${version.id}`, (route) =>
    route.fulfill({ json: { ...version, body: { type: "doc", content: paragraphs } } }),
  );
}

const fontSize = (page: Page, selector: string) =>
  page
    .locator(selector)
    .first()
    .evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize));

test("the editor's text and fields are at least 16 px and Save stays in view", async ({ page }) => {
  await servePage(page);
  await page.goto(`/c/${COURSE}/pages/${PAGE}/edit`);
  const editor = page.getByRole("textbox", { name: "Page text" });
  await expect(editor).toContainText("Paragraph 30");
  expect(await fontSize(page, '[contenteditable="true"]')).toBeGreaterThanOrEqual(16);

  await page.getByRole("button", { name: "Insert" }).tap();
  await page.getByRole("menuitem", { name: "Video" }).tap();
  const panel = page.getByRole("form", { name: "Insert a video" });
  await expect(panel).toBeVisible();
  for (const field of await panel.locator("input").all()) {
    expect(
      await field.evaluate((el) => Number.parseFloat(getComputedStyle(el).fontSize)),
    ).toBeGreaterThanOrEqual(16);
  }
  await panel.getByRole("button", { name: "Cancel" }).tap();

  await editor.getByText("Paragraph 30").scrollIntoViewIfNeeded();
  await expect(page.getByRole("button", { name: "Save" })).toBeInViewport();
});
