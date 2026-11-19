// SPDX-License-Identifier: AGPL-3.0-or-later
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nProvider } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Slider } from "./slider";

describe("Slider", () => {
  it("names the thumb and the number field by its label, with its data-slots", () => {
    const { container } = renderWithStrings(<Slider label="Pass mark" defaultValue={80} />);
    expect(screen.getByRole("slider", { name: "Pass mark" })).toHaveValue("80");
    const input = screen.getByRole("textbox", { name: "Pass mark" });
    expect(input).toHaveValue("80");
    expect(input).toHaveAttribute("data-slot", "slider-input");
    for (const slot of ["slider", "slider-track", "slider-rail", "slider-fill", "slider-thumb"])
      expect(container.querySelector(`[data-slot="${slot}"]`)).not.toBeNull();
  });

  it("takes an exact typed value, which moves the thumb and ends a change", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const onChangeEnd = vi.fn();
    renderWithStrings(
      <Slider label="Pass mark" defaultValue={80} onChange={onChange} onChangeEnd={onChangeEnd} />,
    );
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "73{Enter}");
    expect(screen.getByRole("slider")).toHaveValue("73");
    expect(onChange).toHaveBeenLastCalledWith(73);
    expect(onChangeEnd).toHaveBeenLastCalledWith(73);
  });

  it("keeps a typed value in range and on a step", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Slider label="Pass mark" defaultValue={50} maxValue={100} step={5} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "140{Enter}");
    expect(screen.getByRole("slider")).toHaveValue("100");
    expect(input).toHaveValue("100");
    await user.clear(input);
    await user.type(input, "62{Enter}");
    expect(screen.getByRole("slider")).toHaveValue("60");
  });

  it("puts the value back when the number field is cleared", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Slider label="Pass mark" defaultValue={80} onChange={onChange} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.tab();
    expect(input).toHaveValue("80");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("steps the thumb with the arrow keys, and the number field follows", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Slider label="Pass mark" defaultValue={80} step={5} />);
    await user.tab();
    expect(screen.getByRole("slider")).toHaveFocus();
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(screen.getByRole("textbox")).toHaveValue("90");
  });

  it("starts at the minimum and gives onChange a number when uncontrolled with no default", () => {
    const onChange = vi.fn();
    renderWithStrings(<Slider label="Pass mark" minValue={10} onChange={onChange} />);
    const slider = screen.getByRole("slider");
    expect(slider).toHaveValue("10");
    fireEvent.change(slider, { target: { value: "20" } });
    expect(onChange).toHaveBeenLastCalledWith(20);
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<Slider label="Pass mark" value={80} onChange={onChange} />);
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenCalledWith(81);
    expect(screen.getByRole("slider")).toHaveValue("80");
    expect(screen.getByRole("textbox")).toHaveValue("80");
  });

  it("formats the number field with formatOptions, in the locale", () => {
    renderWithStrings(
      <I18nProvider locale="de-DE">
        <Slider
          label="Pass mark"
          defaultValue={0.75}
          minValue={0}
          maxValue={1}
          step={0.05}
          formatOptions={{ style: "percent" }}
        />
      </I18nProvider>,
    );
    // Intl puts a no-break space before the sign in German.
    expect((screen.getByRole("textbox") as HTMLInputElement).value).toMatch(/^75\s%$/);
  });

  it("names the number field by aria-label without a visible label", () => {
    renderWithStrings(<Slider aria-label="Volume" defaultValue={3} maxValue={10} />);
    expect(screen.getByRole("slider", { name: "Volume" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Volume" })).toBeInTheDocument();
  });

  it("names the number field by aria-labelledby, as the thumb is", () => {
    renderWithStrings(
      <>
        <p id="volume-heading">Volume</p>
        <Slider aria-labelledby="volume-heading" defaultValue={3} maxValue={10} />
      </>,
    );
    expect(screen.getByRole("slider", { name: "Volume" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Volume" })).toBeInTheDocument();
  });

  it("follows a controlled value the caller changes", () => {
    const { rerender } = renderWithStrings(<Slider label="Pass mark" value={80} />);
    rerender(<Slider label="Pass mark" value={60} />);
    expect(screen.getByRole("slider")).toHaveValue("60");
    expect(screen.getByRole("textbox")).toHaveValue("60");
  });

  it("ends a typed change once, on Enter, not again on the Tab after it", async () => {
    const user = userEvent.setup();
    const onChangeEnd = vi.fn();
    renderWithStrings(<Slider label="Pass mark" defaultValue={80} onChangeEnd={onChangeEnd} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "70{Enter}");
    await user.tab();
    expect(onChangeEnd).toHaveBeenCalledTimes(1);
    expect(onChangeEnd).toHaveBeenCalledWith(70);
  });

  it("steps from the number field with ArrowUp, and the thumb follows", async () => {
    const user = userEvent.setup();
    const onChangeEnd = vi.fn();
    renderWithStrings(
      <Slider label="Pass mark" defaultValue={80} step={5} onChangeEnd={onChangeEnd} />,
    );
    await user.click(screen.getByRole("textbox"));
    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("textbox")).toHaveValue("85");
    expect(screen.getByRole("slider")).toHaveValue("85");
    expect(onChangeEnd).toHaveBeenLastCalledWith(85);
  });

  it("focuses the thumb, not the number field, when the label is pressed", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Slider label="Pass mark" defaultValue={80} />);
    await user.click(screen.getByText("Pass mark"));
    expect(screen.getByRole("slider")).toHaveFocus();
  });

  it("joins the caller's aria-labelledby after its label, and passes aria-describedby on", () => {
    renderWithStrings(
      <>
        <p id="unit">percent</p>
        <p id="help">Learners below it retake the exam.</p>
        <Slider
          label="Pass mark"
          aria-labelledby="unit"
          aria-describedby="help"
          defaultValue={80}
        />
      </>,
    );
    for (const control of [screen.getByRole("slider"), screen.getByRole("textbox")]) {
      expect(control).toHaveAccessibleName("Pass mark percent");
      expect(control).toHaveAccessibleDescription("Learners below it retake the exam.");
    }
  });

  it("disables the thumb and the number field together", () => {
    renderWithStrings(<Slider label="Pass mark" defaultValue={80} isDisabled />);
    expect(screen.getByRole("slider")).toBeDisabled();
    expect(screen.getByRole("textbox")).toBeDisabled();
  });

  it("shows no text of its own", () => {
    const { container } = renderWithStrings(<Slider label="«caller»" defaultValue={4} />, {
      marked: true,
    });
    expect(unmarkedText(container).filter((t) => t !== "4")).toEqual([]);
  });
});
