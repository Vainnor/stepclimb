// SPDX-License-Identifier: AGPL-3.0-or-later
// Test support: jsdom has no `matchMedia`. `stubMatchMedia` answers every query with `matches`;
// undo it with `vi.unstubAllGlobals()`.
import { vi } from "vitest";

export function stubMatchMedia(matches: (query: string) => boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: matches(query),
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  }));
}
