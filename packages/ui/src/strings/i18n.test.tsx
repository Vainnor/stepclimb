// SPDX-License-Identifier: AGPL-3.0-or-later
// StringsProvider gives React Aria the UI locale, whatever the browser's language.
import { parseZonedDateTime } from "@internationalized/date";
import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DatePicker } from "../primitives/date-picker";
import { stubMatchMedia } from "../test/media";
import { uiMessages } from "./catalog";
import { StringsProvider } from "./strings";

/**
 * Sets the browser's language and fires `languagechange`, as a browser does when it changes. React
 * Aria listens for it only while one of its components is mounted, so call this after rendering.
 */
function browserLanguage(language: string) {
  vi.spyOn(Navigator.prototype, "language", "get").mockReturnValue(language);
  act(() => {
    window.dispatchEvent(new Event("languagechange"));
  });
}

function renderDue(locale?: string) {
  return render(
    <StringsProvider catalogs={[uiMessages]} locale={locale} isDevelopment={false}>
      <DatePicker
        label="Due date"
        timeZone="America/New_York"
        defaultValue={parseZonedDateTime("2026-03-07T09:00[America/New_York]")}
      />
    </StringsProvider>,
  );
}

const group = () => screen.getByRole("group", { name: "Due date" });
/** What the field shows, without the bidirectional isolation marks Intl puts around the time. */
const shown = () => (group().textContent ?? "").replace(/[\u2066-\u2069]/g, "");

describe("StringsProvider's locale in React Aria", () => {
  beforeEach(() => stubMatchMedia(() => false));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("formats a date field in the UI locale, not the browser's German", () => {
    renderDue();
    browserLanguage("de-DE");
    expect(shown()).toBe("03/07/2026, 09:00 AM EST");
    expect(within(group()).getByRole("spinbutton", { name: /^month/ })).toBeInTheDocument();
  });

  it("formats a date field in a German UI locale in an English browser", () => {
    renderDue("de-DE");
    browserLanguage("en-US");
    expect(shown()).toBe("07.03.2026, 09:00 GMT-5");
    expect(within(group()).getByRole("spinbutton", { name: /^Monat/ })).toBeInTheDocument();
  });
});
