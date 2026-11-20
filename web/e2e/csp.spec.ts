// SPDX-License-Identifier: AGPL-3.0-or-later
// The one report the CSP check leaves out (`csp.ts`, issue #325): WebKit's refusal of Playwright's
// screenshot style. The filter must match that report only, in WebKit, during a screenshot.
import { expect, isScreenshotStyle, type Report, test, WEBKIT_INLINE_STYLE } from "./csp";

const shot = [{ start: 1_000, end: 1_100 }];

function event(at: number, fields: Partial<Report> = {}): Report {
  return {
    at,
    source: "event",
    text: "style-src-elem blocked inline at :5",
    directive: "style-src-elem",
    blocked: "inline",
    sourceFile: "",
    ...fields,
  };
}

function consoleReport(at: number, text = WEBKIT_INLINE_STYLE): Report {
  return { at, source: "console", text };
}

test("the screenshot style's two reports are left out in WebKit during a screenshot", () => {
  expect(isScreenshotStyle(event(1_050), shot, "webkit")).toBe(true);
  expect(isScreenshotStyle(event(1_100, { blocked: "" }), shot, "webkit")).toBe(true);
  expect(isScreenshotStyle(consoleReport(1_003), shot, "webkit")).toBe(true);
});

test("every other report counts", () => {
  const counted: [string, Report, string][] = [
    ["before the screenshot", event(900), "webkit"],
    ["after it returned", event(1_101), "webkit"],
    ["in Chromium", event(1_050), "chromium"],
    ["in Firefox", event(1_050), "firefox"],
    [
      "a style from one of our scripts",
      event(1_050, { sourceFile: "http://x/assets/i.js" }),
      "webkit",
    ],
    ["a script", event(1_050, { directive: "script-src-elem" }), "webkit"],
    ["a style attribute", event(1_050, { directive: "style-src-attr" }), "webkit"],
    ["a stylesheet URL", event(1_050, { blocked: "https://evil.example/x.css" }), "webkit"],
    ["another console text", consoleReport(1_050, "Refused to load http://x/f"), "webkit"],
    ["the console text outside a screenshot", consoleReport(2_000), "webkit"],
  ];
  for (const [name, report, browser] of counted) {
    expect(isScreenshotStyle(report, shot, browser), name).toBe(false);
  }
  expect(isScreenshotStyle(event(1_050), [], "webkit"), "no screenshot").toBe(false);
});
