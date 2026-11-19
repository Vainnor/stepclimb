// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime, type ZonedDateTime } from "@internationalized/date";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WIDE_QUERY } from "../lib/breakpoints";
import { shownText } from "../test/date-text";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { breaksAfter, type DateSegmentValue } from "./date-input";
import { DatePicker } from "./date-picker";

const NY = "America/New_York";
const zoned = (text: string) => parseZonedDateTime(text);

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === WIDE_QUERY ? isWide : false));
}

const group = () => screen.getByRole("group", { name: "Due date" });
/** What the field shows. */
const shown = () => shownText(group());
const segment = (name: RegExp) => within(group()).getByRole("spinbutton", { name });

describe("DatePicker", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows the value with its zone's short name, with its data-slots", () => {
    wide(true);
    const { container } = renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
      />,
    );
    expect(container.querySelector('[data-slot="date-picker"]')).not.toBeNull();
    expect(group()).toHaveAttribute("data-slot", "date-picker-group");
    expect(shown()).toBe("03/07/2026, 09:00 AM EST");
    expect(screen.getByRole("button", { name: /Open calendar/ })).toHaveAttribute(
      "data-slot",
      "date-picker-button",
    );
  });

  it("lets a narrow field wrap only between the date, the time and the zone", () => {
    wide(true);
    const { container } = renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
      />,
    );
    const visible = (el: Element | null) => (el?.textContent ?? "").replace(/[\u2066-\u2069]/g, "");
    // The text of each break literal and of the next segment with any text.
    const breaks = [...container.querySelectorAll('[data-slot="date-break"]')].map((el) => {
      let next = el.nextElementSibling;
      while (next !== null && visible(next) === "") next = next.nextElementSibling;
      return `${visible(el.previousElementSibling)}|${visible(next)}`;
    });
    expect(breaks).toEqual([", |09", " |EST"]);
  });

  it("gives the right instant across the daylight-saving change and says EDT (T3)", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn<(value: ZonedDateTime | null) => void>();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
        onChange={onChange}
      />,
    );
    // 7 March 09:00 is EST (UTC-5); New York moves to EDT (UTC-4) at 02:00 on 8 March.
    await user.click(segment(/^day/));
    await user.keyboard("8");
    await user.tab();
    const value = onChange.mock.lastCall?.[0];
    expect(value?.toString()).toBe("2026-03-08T09:00:00-04:00[America/New_York]");
    expect(value?.toAbsoluteString()).toBe("2026-03-08T13:00:00.000Z");
    expect(shown()).toBe("03/08/2026, 09:00 AM EDT");
    await user.click(segment(/^day/));
    await user.keyboard("7");
    await user.tab();
    expect(onChange.mock.lastCall?.[0]?.toAbsoluteString()).toBe("2026-03-07T14:00:00.000Z");
    expect(shown()).toBe("03/07/2026, 09:00 AM EST");
  });

  it("gives a value in the user's zone when typed into the empty field", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn<(value: ZonedDateTime | null) => void>();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone="Europe/Berlin"
        placeholderValue={zoned("2026-07-01T00:00[Europe/Berlin]")}
        onChange={onChange}
      />,
    );
    await user.click(segment(/^month/));
    await user.keyboard("7152026");
    await user.keyboard("1030P");
    const value = onChange.mock.lastCall?.[0];
    expect(value?.timeZone).toBe("Europe/Berlin");
    expect(value?.toAbsoluteString()).toBe("2026-07-15T20:30:00.000Z");
    expect(shown()).toBe("07/15/2026, 10:30 PM GMT+2");
  });

  it("puts a typed value in timeZone without a placeholderValue", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn<(value: ZonedDateTime | null) => void>();
    renderWithStrings(<DatePicker label="Due date" timeZone="Asia/Tokyo" onChange={onChange} />);
    await user.click(segment(/^month/));
    await user.keyboard("010120301200A");
    await user.tab();
    const value = onChange.mock.lastCall?.[0];
    expect(value?.timeZone).toBe("Asia/Tokyo");
    expect(value?.toAbsoluteString()).toBe("2029-12-31T15:00:00.000Z");
  });

  it("picks a day from the calendar in a Popover, keeping the time and zone", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn<(value: ZonedDateTime | null) => void>();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.closest('[data-slot="calendar-popover"]')).not.toBeNull();
    expect(within(dialog).getByRole("heading")).toHaveTextContent("March 2026");
    expect(within(dialog).getByRole("button", { name: "Previous month" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: /Sunday, March 8, 2026/ }));
    expect(onChange.mock.lastCall?.[0]?.toAbsoluteString()).toBe("2026-03-08T13:00:00.000Z");
  });

  it("opens the calendar in a bottom sheet under 768 px", async () => {
    wide(false);
    const user = userEvent.setup();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.closest('[data-slot="calendar-sheet"]')).not.toBeNull();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("moves through the calendar with the arrow keys", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn<(value: ZonedDateTime | null) => void>();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        defaultValue={zoned("2026-03-07T09:00[America/New_York]")}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Open calendar/ }));
    await user.keyboard("{ArrowRight}{ArrowDown}{Enter}");
    expect(onChange.mock.lastCall?.[0]?.toString()).toBe(
      "2026-03-15T09:00:00-04:00[America/New_York]",
    );
  });

  it("works controlled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        value={zoned("2026-03-07T09:00[America/New_York]")}
        onChange={onChange}
      />,
    );
    await user.click(segment(/^day/));
    await user.keyboard("9");
    await user.tab();
    expect(onChange.mock.lastCall?.[0]?.toString()).toBe(
      "2026-03-09T09:00:00-04:00[America/New_York]",
    );
    // The parent did not take the new value, so the field shows the old one.
    expect(shown()).toBe("03/07/2026, 09:00 AM EST");
  });

  it("shows its error when invalid", () => {
    wide(true);
    renderWithStrings(
      <DatePicker
        label="Due date"
        timeZone={NY}
        isInvalid
        errorMessage="Choose a date after the start date."
      />,
    );
    expect(screen.getByText("Choose a date after the start date.")).toBeInTheDocument();
  });
});

describe("breaksAfter", () => {
  const seg = (type: string, text = type) => ({ type, text }) as DateSegmentValue;
  const lit = (text: string) => seg("literal", text);

  it("breaks between the date and the time and before the zone, not inside the time", () => {
    const segments = [
      seg("month"),
      lit("/"),
      seg("day"),
      lit("/"),
      seg("year"),
      lit(", "),
      seg("hour"),
      lit(":"),
      seg("minute"),
      lit(" "),
      seg("dayPeriod"),
      lit(" "),
      seg("timeZoneName"),
    ];
    const at = segments.flatMap((_, i) => (breaksAfter(segments, i) ? [i] : []));
    expect(at).toEqual([5, 11]);
  });

  it("breaks where a locale puts the time first, and never in a date alone", () => {
    const timeFirst = [
      seg("dayPeriod"),
      lit(" "),
      seg("hour"),
      lit(" "),
      seg("year"),
      lit("."),
      seg("month"),
    ];
    expect(timeFirst.flatMap((_, i) => (breaksAfter(timeFirst, i) ? [i] : []))).toEqual([3]);
    const dateOnly = [seg("month"), lit("/"), seg("day"), lit("/"), seg("year")];
    expect(dateOnly.some((_, i) => breaksAfter(dateOnly, i))).toBe(false);
  });
});
