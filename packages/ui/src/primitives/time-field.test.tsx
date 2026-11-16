// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime } from "@internationalized/date";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { TimeField } from "./time-field";

const zoned = (text: string) => parseZonedDateTime(text);
const group = () => screen.getByRole("group", { name: "Reminder time" });
const shown = () => (group().textContent ?? "").replace(/[⁦-⁩]/g, "");

describe("TimeField", () => {
  it("shows the time with its zone's short name, with its data-slots", () => {
    const { container } = renderWithStrings(
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        defaultValue={zoned("2026-11-16T09:00[America/Chicago]")}
      />,
    );
    expect(container.querySelector('[data-slot="time-field"]')).not.toBeNull();
    expect(group()).toHaveAttribute("data-slot", "date-input");
    expect(shown()).toBe("09:00 AM CST");
  });

  it("gives an instant in the user's zone when typed into the empty field", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        placeholderValue={zoned("2026-07-01T00:00[America/Chicago]")}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("spinbutton", { name: /^hour/ }));
    await user.keyboard("0930A");
    const value = onChange.mock.lastCall?.[0];
    expect(value.timeZone).toBe("America/Chicago");
    expect(value.toAbsoluteString()).toBe("2026-07-01T14:30:00.000Z");
    expect(shown()).toBe("09:30 AM CDT");
  });

  it("steps the hour with the arrow keys, through a daylight-saving change", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        defaultValue={zoned("2026-03-08T01:00[America/Chicago]")}
        onChange={onChange}
      />,
    );
    // Chicago skips from 02:00 CST to 03:00 CDT on 8 March 2026.
    await user.click(screen.getByRole("spinbutton", { name: /^hour/ }));
    await user.keyboard("{ArrowUp}");
    await user.tab();
    expect(onChange.mock.lastCall?.[0].toAbsoluteString()).toBe("2026-03-08T08:00:00.000Z");
    expect(shown()).toBe("03:00 AM CDT");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        value={zoned("2026-11-16T09:00[America/Chicago]")}
        onChange={onChange}
      />,
    );
    await user.click(screen.getByRole("spinbutton", { name: /^hour/ }));
    await user.keyboard("10");
    await user.tab();
    expect(onChange).toHaveBeenCalled();
    expect(shown()).toBe("09:00 AM CST");
  });

  it("shows its error when invalid", () => {
    renderWithStrings(
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        isInvalid
        errorMessage="Choose a time during the working day."
      />,
    );
    expect(screen.getByText("Choose a time during the working day.")).toBeInTheDocument();
    expect(group()).toHaveAttribute("data-invalid", "true");
  });
});
