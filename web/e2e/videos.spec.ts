// SPDX-License-Identifier: AGPL-3.0-or-later
// The video panel's uploader (docs/specs/m1/08-uploaded-video.md B22, T14) in the built app:
// a 20 MiB video goes up in 16 MiB chunks, pauses and resumes, survives a reload when the same file
// is chosen again, and shows processing then ready; axe finds no violation in light, dark and high
// contrast at 1280 and 375 px; and at 375 px on a touch screen every control is 44 px. As in
// files.spec.ts, `/api/v1/me`, the course and the video routes, tus ones included, are answered in
// the browser with synthetic data: the gate's server has no transcoder, so a video never becomes
// ready there.
import { writeFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type Route, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

async function violations(page: Page): Promise<string[]> {
  const result = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  return result.violations.flatMap((v) =>
    v.nodes.map((node) => `${v.id}: ${node.target.join(" ")}: ${node.failureSummary ?? v.help}`),
  );
}

const MIB = 1024 * 1024;
const SIZE = 20 * MIB;
const courseId = "01900000-0000-7000-8000-0000000000c1";
const videoId = "01900000-0000-7000-8000-0000000000e1";
const uploadPath = "/api/v1/video-uploads/01900000-0000-7000-8000-0000000000f1";

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

function video(status: string) {
  return {
    id: videoId,
    source: "upload",
    title: "Week 1",
    status,
    error_code: null,
    duration_s: null,
    chapters: [],
    created_at: "2026-11-02T12:00:00Z",
    updated_at: "2026-11-02T12:00:00Z",
  };
}

const TUS = { "Tus-Resumable": "1.0.0" };

/** The tus server of one test: the upload's offset, the chunks it got, and held chunks. */
class Uploads {
  offset = 0;
  created = false;
  status = "uploading";
  /** The `Upload-Offset` of every `PATCH`, in order. */
  patches: number[] = [];
  /** The next `PATCH` from this offset on waits for `release`. */
  holdFrom = Number.POSITIVE_INFINITY;
  private waiting: ((stored: boolean) => void)[] = [];

  /** Lets the held chunks go: stored, or lost with their connection (nothing stored, D5). */
  release(stored: boolean) {
    for (const go of this.waiting.splice(0)) go(stored);
  }

  get held() {
    return this.waiting.length;
  }

  async patch(route: Route) {
    const request = route.request();
    const at = Number(request.headers()["upload-offset"]);
    this.patches.push(at);
    if (at >= this.holdFrom) {
      const stored = await new Promise<boolean>((go) => this.waiting.push(go));
      if (!stored) return route.abort();
    }
    const size = request.postDataBuffer()?.length ?? 0;
    if (at !== this.offset) {
      return route.fulfill({
        status: 409,
        contentType: "application/problem+json",
        body: JSON.stringify({
          title: "Conflict",
          status: 409,
          code: "video.upload_offset_mismatch",
        }),
      });
    }
    this.offset += size;
    if (this.offset === SIZE) this.status = "processing";
    return route.fulfill({
      status: 204,
      headers: { ...TUS, "Upload-Offset": String(this.offset) },
    });
  }
}

/** A route the page aborted (Pause, a reload) can no longer be answered. */
async function answer(reply: () => Promise<void>) {
  try {
    await reply();
  } catch (error) {
    if (!String(error).includes("aborted") && !String(error).includes("closed")) throw error;
  }
}

async function serveVideos(page: Page, uploads: Uploads, theme = "light") {
  await page.route("**/api/v1/me", (route) => route.fulfill({ json: meWith(theme) }));
  await page.route(`**/api/v1/courses/${courseId}`, (route) => route.fulfill({ json: course }));
  await page.route(`**/api/v1/courses/${courseId}/videos?*`, (route) =>
    route.fulfill({
      json: { items: uploads.created ? [video(uploads.status)] : [], next_cursor: null },
    }),
  );
  await page.route(`**/api/v1/videos/${videoId}`, (route) =>
    route.fulfill({ json: video(uploads.status) }),
  );
  await page.route(`**/api/v1/courses/${courseId}/video-uploads`, (route) => {
    uploads.created = true;
    return route.fulfill({
      status: 201,
      headers: { ...TUS, Location: uploadPath },
      json: { upload_id: uploadPath.split("/").at(-1), video: video("uploading") },
    });
  });
  await page.route(`**${uploadPath}`, (route) =>
    answer(async () => {
      const method = route.request().method();
      if (method === "HEAD") {
        return route.fulfill({
          status: 200,
          headers: {
            ...TUS,
            "Upload-Offset": String(uploads.offset),
            "Upload-Length": String(SIZE),
          },
        });
      }
      if (method === "DELETE") {
        uploads.created = false;
        return route.fulfill({ status: 204, headers: TUS });
      }
      return uploads.patch(route);
    }),
  );
}

/** A 20 MiB file on disk, so its last change stays the same across a reload. */
function videoFile(path: string): string {
  writeFileSync(path, Buffer.alloc(SIZE));
  return path;
}

const fileInput = (page: Page) => page.locator('[data-slot="file-drop"] input[type="file"]');
const card = (page: Page) => page.locator('[data-slot="video-upload"]');

test("the uploader pauses, resumes, and shows processing then ready", async ({ page }, info) => {
  const uploads = new Uploads();
  uploads.holdFrom = 16 * MIB;
  await page.clock.install();
  await serveVideos(page, uploads);
  await page.goto(`/c/${courseId}/videos`);
  await expect(page.getByRole("heading", { level: 1, name: "Videos" })).toBeVisible();

  await fileInput(page).setInputFiles(videoFile(info.outputPath("Week 1.mp4")));
  await expect.poll(() => uploads.held).toBe(1);
  await expect(card(page)).toHaveAttribute("data-phase", "uploading");
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(card(page)).toHaveAttribute("data-phase", "paused");
  await expect(page.getByRole("button", { name: "Resume" })).toBeFocused();
  uploads.release(false);
  expect(uploads.offset).toBe(16 * MIB);

  uploads.holdFrom = Number.POSITIVE_INFINITY;
  await page.getByRole("button", { name: "Resume" }).click();
  await expect(card(page)).toHaveAttribute("data-phase", "processing");
  expect(uploads.patches).toEqual([0, 16 * MIB, 16 * MIB]);
  await expect(page.locator('[data-slot="video-row"]')).toContainText("Processing");

  uploads.status = "ready";
  await page.clock.runFor(10_000);
  await expect(card(page)).toHaveAttribute("data-phase", "ready");
  await expect(card(page).getByText("Week 1.mp4 is ready")).toBeVisible();
  await expect(page.locator('[data-slot="video-row"]')).toContainText("Ready");
});

test("an upload survives a reload when the same file is chosen again", async ({ page }, info) => {
  const uploads = new Uploads();
  uploads.holdFrom = 16 * MIB;
  await serveVideos(page, uploads);
  await page.goto(`/c/${courseId}/videos`);
  const file = videoFile(info.outputPath("Week 1.mp4"));
  await fileInput(page).setInputFiles(file);
  await expect.poll(() => uploads.held).toBe(1);

  await page.reload();
  uploads.release(false);
  uploads.holdFrom = Number.POSITIVE_INFINITY;
  await expect(page.getByRole("heading", { name: "Unfinished uploads" })).toBeVisible();
  await expect(page.getByText("Week 1.mp4: 80% uploaded")).toBeVisible();
  await fileInput(page).setInputFiles(file);
  await expect(card(page)).toHaveAttribute("data-phase", "processing");
  expect(uploads.patches.at(-1)).toBe(16 * MIB);
  expect(uploads.offset).toBe(SIZE);
  await expect(page.getByRole("heading", { name: "Unfinished uploads" })).toHaveCount(0);
});

test.describe("on a touch screen at 375 px", () => {
  test.use({ viewport: { width: 375, height: 740 }, hasTouch: true, isMobile: true });

  test("every control is at least 44 px", async ({ page }, info) => {
    const uploads = new Uploads();
    uploads.holdFrom = 16 * MIB;
    await serveVideos(page, uploads);
    await page.goto(`/c/${courseId}/videos`);
    await fileInput(page).setInputFiles(videoFile(info.outputPath("Week 1.mp4")));
    await expect.poll(() => uploads.held).toBe(1);
    for (const name of ["Choose file", "Pause", "Cancel upload"]) {
      const box = await page.getByRole("button", { name }).boundingBox();
      expect(box?.height, name).toBeGreaterThanOrEqual(44);
      expect(box?.width, name).toBeGreaterThanOrEqual(44);
    }
    uploads.release(false);
  });
});

const themes = ["light", "dark", "high-contrast"] as const;
const widths = [
  { width: 1280, height: 800 },
  { width: 375, height: 740 },
];

for (const theme of themes) {
  for (const viewport of widths) {
    test(`the video panel has no axe violations, ${theme} at ${viewport.width} px`, async ({
      page,
    }, info) => {
      await page.setViewportSize(viewport);
      const uploads = new Uploads();
      uploads.holdFrom = 0;
      await serveVideos(page, uploads, theme);
      await page.goto(`/c/${courseId}/videos`);
      await expect(page.getByText("This course has no videos yet.")).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-sc-theme", theme);
      expect(await violations(page)).toEqual([]);

      await fileInput(page).setInputFiles(videoFile(info.outputPath("Week 1.mp4")));
      await expect.poll(() => uploads.held).toBe(1);
      await page.getByRole("button", { name: "Pause" }).click();
      await expect(card(page)).toHaveAttribute("data-phase", "paused");
      expect(await violations(page)).toEqual([]);
      uploads.release(false);
    });
  }
}
