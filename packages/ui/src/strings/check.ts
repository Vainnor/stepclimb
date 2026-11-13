// SPDX-License-Identifier: AGPL-3.0-or-later
// Catalog checks for each package's Vitest suite: every message lookup in the source uses a
// literal id the English catalogs have, every message parses as ICU, and no English message
// writes an overridable noun as text or puts "a" or "an" before a term argument.
import { IntlMessageFormat } from "intl-messageformat";
import { type Catalog, mergeMessages } from "./catalog";
import { defaultTerms, termArguments } from "./terms";

// A call of `t` whose first argument is a string literal, the form components look messages up in.
const MESSAGE_CALL = /\bt\(\s*(["'])((?:(?!\1)[^\\\n])+)\1/g;
// Any call of `t`, and a string literal at the start of its arguments.
const ANY_CALL = /\bt\(\s*/g;
const LITERAL_ARGUMENT = /^(["'])(?:(?!\1)[^\\\n])+\1/;

/** A source line containing this marker may call `t` with an id that is not a literal. */
export const EXEMPTION = "strings-check-ignore";

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

export interface DynamicLookup {
  file: string;
  line: number;
}

/**
 * Every call of `t` whose first argument is not a string literal (a variable, a template
 * literal), so the missing-id check cannot see its id, unless its line carries `EXEMPTION`.
 */
export function dynamicLookups(sources: Readonly<Record<string, string>>): DynamicLookup[] {
  return Object.entries(sources).flatMap(([file, source]) =>
    Array.from(source.matchAll(ANY_CALL))
      .filter((match) => !LITERAL_ARGUMENT.test(source.slice(match.index + match[0].length)))
      .map((match) => ({ file, line: lineAt(source, match.index) }))
      .filter(({ line }) => !source.split("\n")[line - 1]?.includes(EXEMPTION)),
  );
}

function lineAt(source: string, index: number): number {
  return source.slice(0, index).split("\n").length;
}

type AstElement = ReturnType<IntlMessageFormat["getAst"]>[number];
type ValueElement = AstElement & { value: string };

// Element kinds, numbered as in @formatjs/icu-messageformat-parser's `TYPE` enum.
const LITERAL = 0;
const ARGUMENT = 1;

function isKind(element: AstElement, kind: number): element is ValueElement {
  return element.type === kind && "value" in element && typeof element.value === "string";
}

/** The element lists nested in an element: plural and select option values, tag contents. */
function nestedLists(element: AstElement): AstElement[][] {
  if ("options" in element) return Object.values(element.options).map((option) => option.value);
  if ("children" in element) return [element.children];
  return [];
}

/** Every element list in a message: the top level and each nested one. */
function elementLists(elements: AstElement[]): AstElement[][] {
  return [elements, ...elements.flatMap(nestedLists).flatMap(elementLists)];
}

function englishAsts(catalogs: readonly Catalog[]): [string, AstElement[]][] {
  return Array.from(mergeMessages(catalogs, "en"), ([id, message]) => [
    id,
    new IntlMessageFormat(message, "en").getAst(),
  ]);
}

export interface HardCodedNoun {
  id: string;
  noun: string;
}

/**
 * English messages whose text writes a term's default noun instead of its ICU argument. Only
 * literal text counts, including inside plural and select options; select keys are not text.
 */
export function hardCodedNouns(catalogs: readonly Catalog[]): HardCodedNoun[] {
  const nouns = Object.values(defaultTerms).flatMap((forms) => [forms.one, forms.other]);
  return englishAsts(catalogs).flatMap(([id, ast]) => {
    const text = elementLists(ast)
      .flat()
      .filter((element) => isKind(element, LITERAL))
      .map((element) => (element as ValueElement).value)
      .join(" ");
    return nouns
      .filter((noun) => new RegExp(`\\b${noun}\\b`, "i").test(text))
      .map((noun) => ({ id, noun }));
  });
}

export interface ArticleBeforeTerm {
  id: string;
  term: string;
}

const TRAILING_ARTICLE = /\ban?\s*$/i;

/**
 * English messages that put "a" or "an" directly before a term argument. The article depends on
 * the override ("a course", "an exam"), so such a message must be reworded.
 */
export function articlesBeforeTerms(catalogs: readonly Catalog[]): ArticleBeforeTerm[] {
  const termNames = new Set(Object.keys(termArguments({}, "en")));
  return englishAsts(catalogs).flatMap(([id, ast]) =>
    elementLists(ast)
      .flatMap((list) => termsAfterArticles(list, termNames))
      .map((term) => ({ id, term })),
  );
}

/** The term arguments in one element list that directly follow a literal ending in an article. */
function termsAfterArticles(list: AstElement[], termNames: ReadonlySet<string>): string[] {
  return list.flatMap((element, i) => {
    const next = list[i + 1];
    if (!next || !isKind(next, ARGUMENT) || !termNames.has(next.value)) return [];
    if (!isKind(element, LITERAL) || !TRAILING_ARTICLE.test(element.value)) return [];
    return [next.value];
  });
}

export interface InvalidMessage {
  locale: string;
  id: string;
  error: string;
}

/** Every message, in every locale, that is not valid ICU MessageFormat. */
export function invalidMessages(catalogs: readonly Catalog[]): InvalidMessage[] {
  return catalogs.flatMap(catalogEntries).flatMap(({ locale, id, message }) => {
    const error = parseError(message, locale);
    return error === undefined ? [] : [{ locale, id, error }];
  });
}

function catalogEntries(catalog: Catalog): { locale: string; id: string; message: string }[] {
  return Object.entries(catalog).flatMap(([locale, messages]) =>
    Object.entries(messages ?? {}).map(([id, message]) => ({ locale, id, message })),
  );
}

function parseError(message: string, locale: string): string | undefined {
  try {
    new IntlMessageFormat(message, locale);
    return undefined;
  } catch (error) {
    return String(error);
  }
}
