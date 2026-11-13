// SPDX-License-Identifier: AGPL-3.0-or-later
// Message catalogs: ICU MessageFormat strings by locale, then by message id. Stepclimb ships
// English only at 1.0; English is the locale every other one falls back to, so it is required.

/** ICU messages by id. */
export type Messages = Readonly<Record<string, string>>;

/** One package's messages by locale. Translations (1.x) add locales beside `en`. */
export type Catalog = { readonly en: Messages } & Readonly<Partial<Record<string, Messages>>>;

/** The messages Stepclimb UI's own components render. Each component adds its ids here. */
export const uiMessages: Catalog = {
  en: {
    "link.external": "(external site)",
  },
};

/**
 * Merges the catalogs' messages for one locale. An id defined twice is an error, so the app's
 * catalog cannot silently replace a component's message.
 */
export function mergeMessages(catalogs: readonly Catalog[], locale: string): Map<string, string> {
  const merged = new Map<string, string>();
  for (const catalog of catalogs) {
    for (const [id, message] of Object.entries(catalog[locale] ?? {})) {
      if (merged.has(id)) throw new Error(`message id "${id}" is defined in two catalogs`);
      merged.set(id, message);
    }
  }
  return merged;
}
