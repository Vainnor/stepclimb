// SPDX-License-Identifier: AGPL-3.0-or-later
// Organization terminology: nouns an organization renames, such as Course -> "Training module".
// A message never writes such a noun; it uses the term's ICU arguments, named after the default
// English forms: `{course}` and `{courses}` mid-sentence, `{Course}` and `{Courses}` at the start
// of a sentence. An override replaces the forms, so every message using the term changes together.

/** A term's singular and plural, written as they read mid-sentence. */
export interface TermForms {
  readonly one: string;
  readonly other: string;
}

/** The terms an organization can override, with their English defaults. */
export const defaultTerms = {
  course: { one: "course", other: "courses" },
} as const satisfies Record<string, TermForms>;

export type Term = keyof typeof defaultTerms;

/** An organization's overrides, by term. A term without one keeps its default. */
export type TermOverrides = Partial<Record<Term, TermForms>>;

/** The ICU argument values for every term, with the overrides applied. */
export function termArguments(overrides: TermOverrides, locale: string): Record<string, string> {
  const args: Record<string, string> = {};
  for (const term of Object.keys(defaultTerms) as Term[]) {
    const defaults = defaultTerms[term];
    const forms = overrides[term] ?? defaults;
    args[defaults.one] = forms.one;
    args[defaults.other] = forms.other;
    args[capitalize(defaults.one, locale)] = capitalize(forms.one, locale);
    args[capitalize(defaults.other, locale)] = capitalize(forms.other, locale);
  }
  return args;
}

function capitalize(word: string, locale: string): string {
  const [first = "", ...rest] = word;
  return first.toLocaleUpperCase(locale) + rest.join("");
}
