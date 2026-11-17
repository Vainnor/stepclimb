// SPDX-License-Identifier: AGPL-3.0-or-later
// ServerHtml: HTML the server rendered and sanitized (docs/specs/m1/07-pages-and-the-editor.md
// B14, B22; docs/specs/m1/11-announcements-and-in-app-notifications.md B4), which Prose inserts
// as given. A branded string, so a plain string does not type-check where one is expected: only
// `serverHtml()` makes one, and it is called where an API response's HTML field is parsed.

export type ServerHtml = string & { readonly __brand: "ServerHtml" };

/**
 * Marks `html` as the server's rendered and sanitized HTML. Call it only on an API field the
 * OpenAPI document gives as server-rendered HTML (`Announcement.html`, a page's render), where
 * the response is parsed; never on HTML built, edited or taken from anywhere else in the browser.
 * It checks nothing: the server's sanitizer is what makes the HTML safe to insert.
 */
export function serverHtml(html: string): ServerHtml {
  return html as ServerHtml;
}
