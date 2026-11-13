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
    "alertDialog.cancel": "Cancel",
    "appShell.skipToMain": "Skip to main content",
    "bottomTabs.label": "Main",
    "breadcrumbs.label": "Breadcrumbs",
    "button.pending": "In progress",
    "comboBox.empty": "No matches",
    "comboBox.loading": "Loading",
    "comboBox.showSuggestions": "Show suggestions",
    "copyButton.copied": "Copied",
    "copyButton.failed": "Could not copy. Select the text and copy it yourself.",
    "copyButton.label": "Copy",
    "dialog.close": "Close",
    "field.invalid": "Check what you entered in this field.",
    "field.valueMissing": "Fill in this field.",
    "link.external": "(external site)",
    "meter.value": "{value, number, percent}",
    "numberField.decrement": "Decrease",
    "numberField.increment": "Increase",
    "orgSwitcher.label": "{name}: switch organization",
    "pagination.count": "{count, plural, one {# result} other {# results}}",
    "pagination.label": "Pagination",
    "pagination.next": "Next",
    "pagination.previous": "Previous",
    "progressBar.value": "{value, number, percent}",
    "searchField.clear": "Clear search",
    "select.placeholder": "Choose one",
    "sheet.close": "Close",
    "sideNav.label": "Main",
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
