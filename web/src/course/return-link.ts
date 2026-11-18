// SPDX-License-Identifier: AGPL-3.0-or-later
// A deep link's return URL (docs/specs/m1/03-modules-and-items.md B17, B18, Q2): a host site
// links to `/c/:course/...?return=<url>`, and the course layout shows "Back to <host>" only when
// the URL's origin is one of the organization's `embedding.return_origins` (from `/me`). The value
// is kept for the tab's session in `sessionStorage`, so the link stays while the learner moves
// between the course's pages. Any other value is dropped without a word. The link is followed
// only when the person clicks it; nothing here navigates.
import { useEffect } from "react";

/** The `sessionStorage` key the return URL is kept under. */
export const RETURN_KEY = "stepclimb.return";

/** The longest return URL kept: the longest URL an item may hold (spec 03 B7). */
export const MAX_RETURN_URL = 2048;

export interface ReturnLink {
  /** Where the link goes: the URL as the browser's parser serializes it. */
  href: string;
  /** The host (and port) the link names: the parser's, so it is where the link goes. */
  host: string;
}

/**
 * White space, control and format characters (Unicode Cc, Cf and Z categories). The URL parser
 * drops tabs and newlines and trims spaces, and a format character such as U+202E can make an
 * address read differently from where it goes, so a value holding one is refused rather than
 * cleaned.
 */
const HIDDEN = /[\s\p{Cc}\p{Cf}\p{Z}]/u;

/**
 * `value` as a "Back to" link, or undefined. It must be a string of at most 2,048 characters
 * written as an absolute `https://` URL, with no user name or password and no hidden character,
 * whose origin is exactly one of `origins` (the server keeps them as `https://host[:port]`,
 * lowercase, as `URL.origin` writes them).
 */
export function returnLink(value: unknown, origins: readonly string[]): ReturnLink | undefined {
  if (typeof value !== "string" || value.length > MAX_RETURN_URL) return undefined;
  if (!value.toLowerCase().startsWith("https://") || HIDDEN.test(value)) return undefined;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return undefined;
  }
  if (url.protocol !== "https:" || url.username !== "" || url.password !== "") return undefined;
  if (!origins.includes(url.origin)) return undefined;
  return { href: url.href, host: url.host };
}

/** The tab's kept return URL; undefined when there is none or storage cannot be read. */
function readKept(): string | undefined {
  try {
    return globalThis.sessionStorage.getItem(RETURN_KEY) ?? undefined;
  } catch {
    // A browser with storage turned off throws on access: the link then lives only as long as
    // the address carries it.
    return undefined;
  }
}

function writeKept(value: string | undefined): void {
  try {
    if (value === undefined) globalThis.sessionStorage.removeItem(RETURN_KEY);
    else globalThis.sessionStorage.setItem(RETURN_KEY, value);
  } catch {
    // As in `readKept`: nothing is kept, and the address still carries the value.
  }
}

/**
 * The "Back to" link for the course layout. `fromAddress` is the address's `return` parameter
 * when it has one: it replaces the kept value, which is removed when `fromAddress` makes no link.
 * Without one, the kept value is checked again against `origins`, so an origin the organization
 * removed takes its link away.
 */
export function useReturnLink(
  fromAddress: string | undefined,
  origins: readonly string[],
): ReturnLink | undefined {
  const link = returnLink(fromAddress ?? readKept(), origins);
  const keep = fromAddress !== undefined && link !== undefined;
  useEffect(() => {
    if (fromAddress !== undefined) writeKept(keep ? fromAddress : undefined);
  }, [fromAddress, keep]);
  return link;
}
