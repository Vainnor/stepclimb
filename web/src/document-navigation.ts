// SPDX-License-Identifier: AGPL-3.0-or-later
// A navigation the browser makes itself, leaving the app's page: a link to a server route such as
// a provider's sign-in start (docs/specs/m0/11-app-shell.md B14). Its own module, so a test can
// replace it: jsdom cannot navigate.

/** Loads `href` as a new page, as an ordinary link would. */
export function openDocument(href: string): void {
  globalThis.location.assign(href);
}
