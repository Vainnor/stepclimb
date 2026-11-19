// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: the text a date or time field shows, without the bidirectional isolation marks
// Intl puts around the time and without the zero-width spaces where the field may wrap.

/** `el`'s text as drawn: its segments, leaving out `date-break` spans and isolation marks. */
export function shownText(el: Element): string {
  const copy = el.cloneNode(true) as Element;
  for (const gap of copy.querySelectorAll('[data-slot="date-break"]')) gap.remove();
  return (copy.textContent ?? "").replace(/[⁦-⁩]/g, "");
}
