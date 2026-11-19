// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime } from "@internationalized/date";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WIDE_QUERY } from "../lib/breakpoints";
import { shownText } from "../test/date-text";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { DateRangePicker } from "./date-range-picker";

const NY = "America/New_York";
const zoned = (text: string) => parseZonedDateTime(text);
const range = {
  start: zoned("2026-03-02T09:00[America/New_York]"),
  end: zoned("2026-03-13T17:00[America/New_York]"),
};

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === WIDE_QUERY ? isWide : false));
}

const group = () => screen.getByRole("group", { name: "Open window" });
const shown = () => shownText(group());

describe("DateRangePicker", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows both ends with their zones, with its data-slots", () => {
    wide(true);
    const { container } = renderWithStrings(
      <DateRangePicker label="Open window" timeZone={NY} defaultValue={range} />,
    );
    expect(container.querySelector('[data-slot="date-range-picker"]')).not.toBeNull();
    expect(group()).toHaveAttribute("data-slot", "date-range-picker-group");
    expect(shown()).toBe("03/02/2026, 09:00 AM EST–03/13/2026, 05:00 PM EDT");
    expect(container.querySelectorAll('[data-slot="date-input"]')).toHaveLength(2);
  });

  it("picks a range from the calendar, keeping each end's time and zone", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <DateRangePicker
        label="Open window"
        timeZone={NY}
        defaultValue={range}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: /Thursday, March 5, 2026/ }));
    await user.click(within(dialog).getByRole("button", { name: /Monday, March 9, 2026/ }));
    const value = onChange.mock.lastCall?.[0];
    expect(value.start.toAbsoluteString()).toBe("2026-03-05T14:00:00.000Z");
    expect(value.end.toAbsoluteString()).toBe("2026-03-09T21:00:00.000Z");
  });

  it("gives a range in the user's zone when typed into the empty field", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <DateRangePicker
        label="Open window"
        timeZone="Asia/Tokyo"
        placeholderValue={zoned("2026-05-01T00:00[Asia/Tokyo]")}
        onChange={onChange}
      />,
    );
    const months = within(group()).getAllByRole("spinbutton", { name: /^month/ });
    await user.click(months[0] as HTMLElement);
    await user.keyboard("5012026900A");
    await user.click(months[1] as HTMLElement);
    await user.keyboard("5022026500P");
    await user.tab();
    const value = onChange.mock.lastCall?.[0];
    expect(value.start.timeZone).toBe("Asia/Tokyo");
    expect(value.start.toAbsoluteString()).toBe("2026-05-01T00:00:00.000Z");
    expect(value.end.toAbsoluteString()).toBe("2026-05-02T08:00:00.000Z");
  });

  it("opens the calendar in a bottom sheet under 768 px", async () => {
    wide(false);
    const user = userEvent.setup();
    renderWithStrings(<DateRangePicker label="Open window" timeZone={NY} defaultValue={range} />);
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    expect(screen.getByRole("dialog").closest('[data-slot="calendar-sheet"]')).not.toBeNull();
  });

  it("works controlled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <DateRangePicker label="Open window" timeZone={NY} value={range} onChange={onChange} />,
    );
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: /Thursday, March 5, 2026/ }));
    await user.click(within(dialog).getByRole("button", { name: /Monday, March 9, 2026/ }));
    expect(onChange).toHaveBeenCalled();
    expect(shown()).toBe("03/02/2026, 09:00 AM EST–03/13/2026, 05:00 PM EDT");
  });
});
