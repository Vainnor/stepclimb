// SPDX-License-Identifier: AGPL-3.0-or-later
// The link targets a page may hold (docs/specs/m1/07-pages-and-the-editor.md B9), checked in the
// browser the way `stepclimb_core::page_doc::link_target` checks them on the server, so the editor
// can refuse a bad target as it is typed. The server checks every save again and is the authority;
// crates/stepclimb-core/tests/fixtures/page_doc/link-targets.json holds the cases both sides run.

/** The longest absolute URL, in characters (`items::MAX_URL_CHARS`). */
const MAX_URL_CHARS = 2048;

/** The longest same-origin path, in bytes (`auth_flows::MAX_RETURN_TO_LEN`). */
const MAX_PATH_BYTES = 2048;

/** Characters no stored target may hold: they could end an HTML attribute or start markup. */
const REFUSED = /["'<>`\\\p{Cc}]/u;

/** White space, control and format characters, which no absolute URL may hold. */
const NOT_PLAIN = /[\s\p{Cc}\p{Cf}]/u;

/**
 * `href` in the form the server stores it, or `null` when the server would refuse it: an
 * absolute `https`, `http` or `mailto` URL as the URL parser serializes it (scheme and host
 * lowercased, some characters percent-encoded), or a path on the same origin as given.
 */
export function linkTarget(href: string): string | null {
  const stored = storedForm(href);
  return stored === null || REFUSED.test(stored) ? null : stored;
}

function storedForm(href: string): string | null {
  if (href.startsWith("/")) return isSafePath(href) ? href : null;
  const valid =
    href.slice(0, 7).toLowerCase() === "mailto:" ? mailtoIsValid(href) : urlIsValid(href);
  return valid ? (parse(href)?.href ?? null) : null;
}

function parse(href: string): URL | null {
  try {
    return new URL(href);
  } catch {
    return null;
  }
}

/**
 * A path on this origin (spec m0/08 B19): one leading `/` not followed by `/` or `\`, printable
 * ASCII only, no `.` or `..` segment (also written `%2e`), at most 2,048 bytes.
 */
function isSafePath(path: string): boolean {
  return (
    path.length <= MAX_PATH_BYTES &&
    path[1] !== "/" &&
    path[1] !== "\\" &&
    /^[!-~]+$/.test(path) &&
    !hasDotSegment(path)
  );
}

function hasDotSegment(path: string): boolean {
  const end = path.search(/[?#]/);
  return (end === -1 ? path : path.slice(0, end)).split(/[/\\]/).some((segment) => {
    const decoded = segment.toLowerCase().replaceAll("%2e", ".");
    return decoded === "." || decoded === "..";
  });
}

/** An absolute `http` or `https` URL with a host and no user name or password. */
function urlIsValid(href: string): boolean {
  const lower = href.slice(0, 8).toLowerCase();
  if (!lower.startsWith("https://") && !lower.startsWith("http://")) return false;
  if (NOT_PLAIN.test(href) || [...href].length > MAX_URL_CHARS) return false;
  const url = parse(href);
  return (
    url !== null &&
    (url.protocol === "https:" || url.protocol === "http:") &&
    url.hostname !== "" &&
    url.username === "" &&
    url.password === ""
  );
}

/** A `mailto:` URL with an address part. */
function mailtoIsValid(href: string): boolean {
  return (
    !NOT_PLAIN.test(href) &&
    [...href].length <= MAX_URL_CHARS &&
    href.length > "mailto:".length &&
    parse(href)?.protocol === "mailto:"
  );
}
