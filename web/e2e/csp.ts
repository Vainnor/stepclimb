// SPDX-License-Identifier: AGPL-3.0-or-later
// Playwright's `test` with one more automatic check: no page of the test reports a
// Content-Security-Policy violation (issue #325). Every spec imports `test` and `expect` from here.
// The server sends the policy with every page of the web app (`crates/stepclimb-server/src/spa.rs`),
// so a script, style, font, frame or connection the policy refuses fails the test that met it,
// whichever route it visited. The browser reports a violation twice, as a
// `securitypolicyviolation` event in the page and as a console message; both are collected, so a
// page whose scripts are turned off still reports through the console. Contexts a test opens
// itself with `browser.newContext` are watched too.
import { type BrowserContext, test as base, expect } from "@playwright/test";

export type { Browser, Page, Request, Route } from "@playwright/test";
export { expect };

/** The binding the init script reports a violation through. */
const BINDING = "__scCspViolation";

/** Collect into `violations` every policy violation the pages of `context` report. */
async function watch(context: BrowserContext, violations: string[]): Promise<void> {
  await context.exposeBinding(BINDING, (_source, text: string) => {
    violations.push(text);
  });
  await context.addInitScript((binding) => {
    document.addEventListener("securitypolicyviolation", (event) => {
      const report = (window as unknown as Record<string, (text: string) => void>)[binding];
      report?.(
        `${event.effectiveDirective} blocked ${event.blockedURI || "inline"} at ${event.sourceFile}:${event.lineNumber}`,
      );
    });
  }, BINDING);
  context.on("console", (message) => {
    if (/content.security.policy/i.test(message.text())) violations.push(message.text());
  });
}

export const test = base.extend<{ cspViolations: string[] }>({
  cspViolations: [
    async ({ context, browser }, use) => {
      const violations: string[] = [];
      await watch(context, violations);
      const newContext = browser.newContext.bind(browser);
      browser.newContext = async (options) => {
        const opened = await newContext(options);
        await watch(opened, violations);
        return opened;
      };
      try {
        await use(violations);
      } finally {
        browser.newContext = newContext;
      }
      expect(violations, "Content-Security-Policy violations").toEqual([]);
    },
    { auto: true },
  ],
});
