// SPDX-License-Identifier: AGPL-3.0-or-later
// Playwright's `test` with one more automatic check: no page of the test reports a
// Content-Security-Policy violation (issue #325). Every spec imports `test` and `expect` from here.
// The server sends the policy with every page of the web app (`crates/stepclimb-server/src/spa.rs`),
// so a script, style, font, frame or connection the policy refuses fails the test that met it,
// whichever route it visited. The browser reports a violation twice, as a
// `securitypolicyviolation` event in the page and as a console message; both are collected, so a
// page whose scripts are turned off still reports through the console. Contexts a test opens
// itself with `browser.newContext` are watched too.
//
// Two reports are left out, each because the policy it names is not the web app's or the code it
// blocks is not the page's:
// - a signed file (`/files/<token>`) shown in a frame runs under the file's own policy
//   (`file_delivery.rs`), so reports from its document are not counted;
// - in WebKit, `page.screenshot` adds `<style>body {}</style>` to every frame to restart CSS
//   animations (playwright-core `inPagePrepareForScreenshots`, `syncAnimations`). The page's
//   policy refuses it, as it should, and WebKit reports an inline `style-src-elem` violation with
//   no source file. [`isScreenshotStyle`] matches only that report, only in WebKit and only while
//   a screenshot is being taken; `csp.spec.ts` checks it matches nothing else.
import { type BrowserContext, test as base, expect, type Page } from "@playwright/test";

export type { Browser, Page, Request, Route } from "@playwright/test";
export { expect };

/** The binding the init script reports a violation through. */
const BINDING = "__scCspViolation";

/** The console text WebKit writes when it refuses an inline `<style>`. */
export const WEBKIT_INLINE_STYLE =
  "Refused to apply a stylesheet because its hash, its nonce, or 'unsafe-inline' does not appear in the style-src directive of the Content Security Policy.";

/** One violation a page reported, with when the test received it (`Date.now()`). */
export interface Report {
  at: number;
  /** A `securitypolicyviolation` event's fields, or a console message's text. */
  source: "event" | "console";
  text: string;
  directive?: string;
  blocked?: string;
  sourceFile?: string;
}

/** When a screenshot was being taken, in `Date.now()` time. */
export interface Window {
  start: number;
  end: number;
}

/** How long after a screenshot returns its report may still arrive, in milliseconds. */
const GRACE_MS = 250;

/**
 * Whether `report` is the one WebKit raises for Playwright's own screenshot style: received while
 * a screenshot was being taken, in WebKit, and either an inline `style-src-elem` event with no
 * source file or WebKit's console text for a refused inline style.
 */
export function isScreenshotStyle(report: Report, windows: Window[], browserName: string): boolean {
  if (browserName !== "webkit") return false;
  const during = windows.some((w) => report.at >= w.start && report.at <= w.end + GRACE_MS);
  if (!during) return false;
  if (report.source === "console") return report.text === WEBKIT_INLINE_STYLE;
  const inline = report.blocked === "inline" || report.blocked === "";
  return report.directive === "style-src-elem" && inline && report.sourceFile === "";
}

/** Collect into `reports` every policy violation the pages of `context` report. */
async function watch(context: BrowserContext, reports: Report[], windows: Window[]) {
  await context.exposeBinding(BINDING, (_source, event: Omit<Report, "at" | "source">) => {
    reports.push({ ...event, at: Date.now(), source: "event" });
  });
  await context.addInitScript((binding) => {
    if (location.pathname.startsWith("/files/")) return;
    document.addEventListener("securitypolicyviolation", (event) => {
      const report = (window as unknown as Record<string, (event: object) => void>)[binding];
      report?.({
        text: `${event.effectiveDirective} blocked ${event.blockedURI || "inline"} at ${event.sourceFile}:${event.lineNumber}`,
        directive: event.effectiveDirective,
        blocked: event.blockedURI,
        sourceFile: event.sourceFile,
      });
    });
  }, BINDING);
  context.on("console", (message) => {
    if (!/content.security.policy/i.test(message.text())) return;
    if (fromSignedFile(message.location().url)) return;
    reports.push({ at: Date.now(), source: "console", text: message.text() });
  });
  for (const page of context.pages()) timeScreenshots(page, windows);
  context.on("page", (page) => timeScreenshots(page, windows));
}

/** Record in `windows` when each `page.screenshot` call runs. */
function timeScreenshots(page: Page, windows: Window[]) {
  const screenshot = page.screenshot.bind(page);
  page.screenshot = async (options) => {
    const start = Date.now();
    try {
      return await screenshot(options);
    } finally {
      windows.push({ start, end: Date.now() });
    }
  };
}

/** Whether `url` is a signed file's document, which runs under the file's own policy. */
function fromSignedFile(url: string): boolean {
  return url !== "" && new URL(url).pathname.startsWith("/files/");
}

export const test = base.extend<{ cspViolations: Report[] }>({
  cspViolations: [
    async ({ context, browser, browserName }, use) => {
      const reports: Report[] = [];
      const windows: Window[] = [];
      await watch(context, reports, windows);
      const newContext = browser.newContext.bind(browser);
      browser.newContext = async (options) => {
        const opened = await newContext(options);
        await watch(opened, reports, windows);
        return opened;
      };
      try {
        await use(reports);
      } finally {
        browser.newContext = newContext;
      }
      const counted = reports.filter((r) => !isScreenshotStyle(r, windows, browserName));
      expect(
        counted.map((r) => r.text),
        "Content-Security-Policy violations",
      ).toEqual([]);
    },
    { auto: true },
  ],
});
