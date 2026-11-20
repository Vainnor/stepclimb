// SPDX-License-Identifier: AGPL-3.0-or-later
// The account page's accessibility (docs/specs/m0/04-local-accounts.md B13; 11-app-shell.md T19):
// the passkey list, the add dialog and the delete confirmation in light, dark and high contrast at
// 1280 and 375 px. As in states.spec.ts, `/api/v1/me` and the passkey routes are answered in the
// browser with synthetic data; sign-in.spec.ts drives the same page against the real server.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "./csp";

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

const passkeys = [
  {
    id: "01900000-0000-7000-8000-0000000000a1",
    name: "Work laptop",
    created_at: "2026-11-02T12:00:00Z",
    last_used_at: "2026-11-10T12:00:00Z",
  },
  {
    id: "01900000-0000-7000-8000-0000000000a2",
    name: "Phone",
    created_at: "2026-11-05T12:00:00Z",
    last_used_at: null,
  },
];

/** Answers `/me`, the passkey list and the add route's start in the browser. */
async function serveAccount(page: Page, theme: string) {
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route("**/api/v1/me/passkeys", (route) => route.fulfill({ json: passkeys }));
  await page.route("**/api/v1/me/passkeys/start", (route) =>
    route.fulfill({
      json: { options: { publicKey: { challenge: "Y2g", rp: { id: "localhost", name: "x" } } } },
    }),
  );
}

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the account page has no axe violations, ${theme} at ${viewport.width} px`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await serveAccount(page, theme);
      await page.goto("/account");
      await expect(page.getByRole("heading", { level: 1, name: "Account" })).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      await expect(page.getByRole("row")).toHaveCount(2);
      expect(await violations(page)).toEqual([]);

      await page.getByRole("button", { name: "Delete Work laptop" }).click();
      const confirm = page.getByRole("alertdialog", { name: "Delete Work laptop?" });
      await expect(confirm).toBeVisible();
      expect(await violations(page)).toEqual([]);
      await confirm.getByRole("button", { name: "Cancel" }).click();
      await expect(confirm).toBeHidden();

      await page.getByRole("button", { name: "Add a passkey" }).click();
      const add = page.getByRole("dialog", { name: "Add a passkey" });
      await expect(add.getByRole("button", { name: "Create the passkey" })).toBeEnabled();
      expect(await violations(page)).toEqual([]);
    });
  }
}
