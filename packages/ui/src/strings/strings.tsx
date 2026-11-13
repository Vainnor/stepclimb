// SPDX-License-Identifier: AGPL-3.0-or-later
// `useStepclimbStrings()`: every user-visible string is an ICU message looked up by id
// (docs/specs/m0/11-app-shell.md B23 to B26). The provider takes the catalogs of Stepclimb UI and
// of the app, the locale and the organization's terminology overrides.
import { IntlMessageFormat, type PrimitiveType } from "intl-messageformat";
import { createContext, type ReactNode, use, useMemo } from "react";
import { type Catalog, mergeMessages } from "./catalog";
import { type TermOverrides, termArguments } from "./terms";

/** Values for a message's ICU arguments: `{count, plural, ...}`, `{when, date, medium}`. */
export type MessageValues = Readonly<Record<string, PrimitiveType>>;

export interface Strings {
  /**
   * Formats the message `id`. Term arguments (`{course}`) are filled in by the provider. Every
   * other argument the message names must be in `values`; a missing one throws.
   */
  t: (id: string, values?: MessageValues) => string;
}

export interface StringsOptions {
  catalogs: readonly Catalog[];
  /** The UI locale; `en` is the only one at 1.0. */
  locale?: string;
  terms?: TermOverrides;
  /**
   * What an id missing from the locale's catalogs renders: the id itself in development, so the
   * gap is visible; the English message in production. An id English lacks renders as the id.
   */
  isDevelopment: boolean;
}

export function createStrings({
  catalogs,
  locale = "en",
  terms = {},
  isDevelopment,
}: StringsOptions): Strings {
  const active = mergeMessages(catalogs, locale);
  const english = locale === "en" ? active : mergeMessages(catalogs, "en");
  const termArgs = termArguments(terms, locale);
  const compiled = new Map<string, IntlMessageFormat>();

  function formatter(id: string): IntlMessageFormat | undefined {
    const cached = compiled.get(id);
    if (cached) return cached;
    const message = active.get(id);
    const fallback = isDevelopment ? undefined : english.get(id);
    let format: IntlMessageFormat | undefined;
    if (message !== undefined) format = new IntlMessageFormat(message, locale);
    else if (fallback !== undefined) format = new IntlMessageFormat(fallback, "en");
    if (format) compiled.set(id, format);
    return format;
  }

  function lookup(id: string, values: MessageValues = {}): string {
    for (const key of Object.keys(values)) {
      if (Object.hasOwn(termArgs, key)) {
        throw new Error(`message "${id}": the value "${key}" would replace a term argument`);
      }
    }
    const format = formatter(id);
    if (!format) return id;
    const out = format.format<string>({ ...termArgs, ...values });
    // Only primitive values are passed, so the formatter returns one string.
    if (typeof out !== "string") throw new Error(`message "${id}" did not format to a string`);
    return out;
  }

  return { t: lookup };
}

const StringsContext = createContext<Strings | null>(null);

export interface StringsProviderProps extends StringsOptions {
  children: ReactNode;
}

export function StringsProvider({
  catalogs,
  locale,
  terms,
  isDevelopment,
  children,
}: StringsProviderProps) {
  const strings = useMemo(
    () => createStrings({ catalogs, locale, terms, isDevelopment }),
    [catalogs, locale, terms, isDevelopment],
  );
  return <StringsContext value={strings}>{children}</StringsContext>;
}

export function useStepclimbStrings(): Strings {
  const strings = use(StringsContext);
  if (!strings) throw new Error("useStepclimbStrings() needs a <StringsProvider> above it");
  return strings;
}
