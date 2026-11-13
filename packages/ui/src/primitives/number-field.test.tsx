// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nProvider } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { NumberField } from "./number-field";

describe("NumberField", () => {
  it("labels its input, with tabular numerals and its data-slots", () => {
    renderWithStrings(<NumberField label="Pass mark" defaultValue={80} />);
    const input = screen.getByRole("textbox", { name: "Pass mark" });
    expect(input).toHaveAttribute("data-slot", "number-field-input");
    expect(input).toHaveClass("tabular-nums");
    expect(input).toHaveValue("80");
    expect(input.closest('[data-slot="number-field-group"]')).not.toBeNull();
  });

  it("names its step buttons from the catalog and steps with them", async () => {
    const user = userEvent.setup();
    renderWithStrings(<NumberField label="Attempts" defaultValue={3} />);
    await user.click(screen.getByRole("button", { name: /^Increase/ }));
    expect(screen.getByRole("textbox")).toHaveValue("4");
    await user.click(screen.getByRole("button", { name: /^Decrease/ }));
    await user.click(screen.getByRole("button", { name: /^Decrease/ }));
    expect(screen.getByRole("textbox")).toHaveValue("2");
  });

  it("steps with the arrow keys, within its range", async () => {
    const user = userEvent.setup();
    renderWithStrings(<NumberField label="Attempts" defaultValue={2} minValue={1} maxValue={3} />);
    await user.click(screen.getByRole("textbox"));
    await user.keyboard("{ArrowUp}{ArrowUp}{ArrowUp}");
    expect(screen.getByRole("textbox")).toHaveValue("3");
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
    expect(screen.getByRole("textbox")).toHaveValue("1");
  });

  it("keeps the step buttons out of the tab order", async () => {
    const user = userEvent.setup();
    renderWithStrings(<NumberField label="Attempts" />);
    await user.tab();
    expect(screen.getByRole("textbox")).toHaveFocus();
    await user.tab();
    expect(document.body).toHaveFocus();
  });

  it("formats in the locale through React Aria", () => {
    renderWithStrings(
      <I18nProvider locale="de-DE">
        <NumberField label="Distance" defaultValue={1234.5} />
      </I18nProvider>,
    );
    expect(screen.getByRole("textbox")).toHaveValue("1.234,5");
  });

  it("formats with formatOptions", () => {
    renderWithStrings(
      <NumberField label="Pass mark" defaultValue={0.8} formatOptions={{ style: "percent" }} />,
    );
    expect(screen.getByRole("textbox")).toHaveValue("80%");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<NumberField label="Attempts" value={3} onChange={onChange} />);
    await user.click(screen.getByRole("button", { name: /^Increase/ }));
    expect(onChange).toHaveBeenCalledWith(4);
    expect(screen.getByRole("textbox")).toHaveValue("3");
  });

  it("names its buttons only from the strings catalog (T2)", () => {
    const { container } = renderWithStrings(<NumberField label="«caller»" defaultValue={1} />, {
      marked: true,
    });
    // React Aria adds the field's label to each button's name: "Increase Pass mark".
    expect(
      screen.getByRole("button", { name: "«numberField.increment» «caller»" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "«numberField.decrement» «caller»" }),
    ).toBeInTheDocument();
    expect(unmarkedText(container).filter((t) => t !== "1")).toEqual([]);
  });
});
