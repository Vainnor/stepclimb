// SPDX-License-Identifier: AGPL-3.0-or-later
// Catalog checks for each package's Vitest suite: every message id the source uses exists in the
// English catalogs, every message parses as ICU, and no English message hard-codes a noun that is
// an overridable term.
import { IntlMessageFormat } from "intl-messageformat";
import { type Catalog, mergeMessages } from "./catalog";
import { defaultTerms } from "./terms";

// A call of `t` whose first argument is a string literal, the form components look messages up in.
const MESSAGE_CALL = /\bt\(\s*(["'])((?:(?!\1)[^\\\n])+)\1/g;

/** The message ids a source file passes to `t` as string literals. */
export function findMessageIds(source: string): string[] {
  return Array.from(source.matchAll(MESSAGE_CALL), (match) => match[2] ?? "");
}

export interface MissingId {
  file: string;
  id: string;
}

/** Every literal id in `sources` (file path -> contents) that the English catalogs lack. */
export function missingMessageIds(
  sources: Readonly<Record<string, string>>,
  catalogs: readonly Catalog[],
): MissingId[] {
  const english = mergeMessages(catalogs, "en");
  return Object.entries(sources).flatMap(([file, source]) =>
    findMessageIds(source)
      .filter((id) => !english.has(id))
      .map((id) => ({ file, id })),
  );
}

export interface HardCodedNoun {
  id: string;
  noun: string;
}

// A plain argument reference such as `{course}`; term arguments are the allowed way to write one.
const ARGUMENT = /\{\s*[A-Za-z_]\w*\s*\}/g;

/** English messages that write a term's default noun as text instead of its ICU argument. */
export function hardCodedNouns(catalogs: readonly Catalog[]): HardCodedNoun[] {
  const nouns = Object.values(defaultTerms).flatMap((forms) => [forms.one, forms.other]);
  const found: HardCodedNoun[] = [];
  for (const [id, message] of mergeMessages(catalogs, "en")) {
    const text = message.replace(ARGUMENT, " ");
    for (const noun of nouns) {
      if (new RegExp(`\\b${noun}\\b`, "i").test(text)) found.push({ id, noun });
    }
  }
  return found;
}

export interface InvalidMessage {
  locale: string;
  id: string;
  error: string;
}

/** Every message, in every locale, that is not valid ICU MessageFormat. */
export function invalidMessages(catalogs: readonly Catalog[]): InvalidMessage[] {
  const found: InvalidMessage[] = [];
  for (const catalog of catalogs) {
    for (const [locale, messages] of Object.entries(catalog)) {
      for (const [id, message] of Object.entries(messages ?? {})) {
        try {
          new IntlMessageFormat(message, locale);
        } catch (error) {
          found.push({ locale, id, error: String(error) });
        }
      }
    }
  }
  return found;
}
