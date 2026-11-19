// SPDX-License-Identifier: AGPL-3.0-or-later
// The course authoring screen's Pages tab on a phone (docs/specs/m1/16-learning-ui-components.md
// B2, B19): each page's link covers its row, so the target is at least 44 px tall at 375 px with a
// coarse pointer. As in videos.spec.ts, `/api/v1/me`, the course and its pages are answered in the
// browser with synthetic data; the real-stack journey is in sign-in.spec.ts.
import { expect, test } from "@playwright/test";

const courseId = "01900000-0000-7000-8000-0000000000c1";
const STAMP = "2026-11-02T12:00:00Z";

const me = {
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
  appearance: { theme: "light", density: "comfortable" },
  default_appearance: "system",
};

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
  created_at: STAMP,
  updated_at: STAMP,
};

const page = {
  id: "01900000-0000-7000-8000-0000000000p1",
  course_id: courseId,
  title: "Taxi",
  slug: "taxi",
  current_version: {
    id: "01900000-0000-7000-8000-0000000000v1",
    number: 2,
    author_id: me.user.id,
    created_at: STAMP,
  },
  created_at: STAMP,
  updated_at: STAMP,
};

test.use({ viewport: { width: 375, height: 740 }, hasTouch: true, isMobile: true });

test("a page's row is its link's target, at least 44 px tall at 375 px", async ({ page: tab }) => {
  await tab.route("**/api/v1/me", (route) => route.fulfill({ json: me }));
  await tab.route(`**/api/v1/courses/${courseId}`, (route) => route.fulfill({ json: course }));
  await tab.route(`**/api/v1/courses/${courseId}/modules`, (route) =>
    route.fulfill({ json: { modules: [] } }),
  );
  await tab.route(`**/api/v1/courses/${courseId}/pages?*`, (route) =>
    route.fulfill({ json: { items: [page], next_cursor: null } }),
  );
  await tab.goto(`/c/${courseId}/edit?panel=pages`);
  const link = tab.getByRole("link", { name: "Taxi" });
  await expect(link).toBeVisible();
  const row = tab.locator('[data-slot="page-row"]');
  const box = await row.boundingBox();
  expect(box?.height).toBeGreaterThanOrEqual(44);
  // A press anywhere on the row, the details line under the title included, lands on the link.
  const hits = await row.evaluate((li) => {
    const r = li.getBoundingClientRect();
    return [r.top + 2, r.top + r.height / 2, r.bottom - 2].map(
      (y) => document.elementFromPoint(r.left + r.width / 2, y)?.closest("a")?.textContent ?? null,
    );
  });
  expect(hits).toEqual(["Taxi", "Taxi", "Taxi"]);
});
