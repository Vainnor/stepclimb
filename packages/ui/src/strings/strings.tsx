// SPDX-License-Identifier: AGPL-3.0-or-later
// `useStepclimbStrings()`: every user-visible string is an ICU message looked up by id
// (docs/specs/m0/11-app-shell.md B23 to B26). The provider takes the catalogs of Stepclimb UI and
// of the app, the locale and the organization's terminology overrides, and gives React Aria the
// same locale.
import { IntlMessageFormat } from "intl-messageformat";
import { createContext, type ReactNode, use, useMemo, useSyncExternalStore } from "react";
import { I18nProvider } from "react-aria-components";
import { type Catalog, mergeMessages } from "./catalog";
import { defaultTerms, type TermOverrides, termArguments } from "./terms";

/**
 * Values for a message's ICU arguments: `{count, plural, ...}`, `{when, date, medium}`. There is
 * no null or undefined: a missing value would render as nothing, so lookups refuse one.
 */
export type MessageValues = Readonly<Record<string, string | number | bigint | boolean | Date>>;

export interface Strings {
  /**
   * Formats the message `id`. Term arguments (`{course}`) are filled in by the provider. Every
   * other argument the message names must be in `values`; a missing, null or undefined one
   * throws.
   */
  t: (id: string, values?: MessageValues) => string;
}

/** The UI locale when none is given. */
export const DEFAULT_LOCALE = "en";

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
  locale = DEFAULT_LOCALE,
  terms = {},
  isDevelopment,
}: StringsOptions): Strings {
  const active = mergeMessages(catalogs, locale);
  const english = locale === "en" ? active : mergeMessages(catalogs, "en");
  const termArgs = termArguments(defaultTerms, terms, locale);
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
    for (const [key, value] of Object.entries(values)) {
      if (Object.hasOwn(termArgs, key)) {
        throw new Error(`message "${id}": the value "${key}" would replace a term argument`);
      }
      if (value === null || value === undefined) {
        throw new Error(`message "${id}": the value "${key}" is ${value}`);
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

/** The language subtag of a locale tag ("en" for "en-GB"), or undefined for a malformed tag. */
function languageOf(tag: string): string | undefined {
  try {
    return new Intl.Locale(tag).language;
  } catch {
    return undefined;
  }
}

/**
 * React Aria's locale: the browser's (`navigator.language`) when it is in the UI's language, so an
 * English UI in a British browser writes dates the British way; otherwise the UI locale, so an
 * English UI in a German browser stays English.
 */
export function reactAriaLocale(uiLocale: string, browserLocale: string | undefined): string {
  if (browserLocale === undefined) return uiLocale;
  const browser = languageOf(browserLocale);
  return browser !== undefined && browser === languageOf(uiLocale) ? browserLocale : uiLocale;
}

function subscribeToLanguage(onChange: () => void): () => void {
  window.addEventListener("languagechange", onChange);
  return () => window.removeEventListener("languagechange", onChange);
}

/** The browser's language, followed as it changes; undefined when rendered on a server. */
function useBrowserLocale(): string | undefined {
  return useSyncExternalStore(
    subscribeToLanguage,
    () => navigator.language,
    () => undefined,
  );
}

/**
 * Provides `useStepclimbStrings()`, and React Aria's locale through its `I18nProvider`: date and
 * number formats, segment names and drag-and-drop announcements are in the UI's language, in the
 * browser's region when the two share a language (`reactAriaLocale`).
 */
export function StringsProvider({
  catalogs,
  locale = DEFAULT_LOCALE,
  terms,
  isDevelopment,
  children,
}: StringsProviderProps) {
  const strings = useMemo(
    () => createStrings({ catalogs, locale, terms, isDevelopment }),
    [catalogs, locale, terms, isDevelopment],
  );
  const ariaLocale = reactAriaLocale(locale, useBrowserLocale());
  return (
    <StringsContext value={strings}>
      <I18nProvider locale={ariaLocale}>{children}</I18nProvider>
    </StringsContext>
  );
}

export function useStepclimbStrings(): Strings {
  const strings = use(StringsContext);
  if (!strings) throw new Error("useStepclimbStrings() needs a <StringsProvider> above it");
  return strings;
}
