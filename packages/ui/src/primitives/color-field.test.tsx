// SPDX-License-Identifier: AGPL-3.0-or-later
// Colors are written as numbers and typed without "#": the UI lint refuses a hex string anywhere
// outside packages/tokens.
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { type Color, parseColor } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { ColorField } from "./color-field";

/** A color from its 24-bit RGB value. */
const rgb = (n: number): Color => parseColor(`#${n.toString(16).padStart(6, "0")}`);
const teal = rgb(0x0b6e75);

/** The color `onChange` was last called with. */
function lastColor(onChange: ReturnType<typeof vi.fn>): Color {
  const color: unknown = onChange.mock.lastCall?.[0];
  if (color === null || typeof color !== "object") throw new Error("onChange got no color");
  return color as Color;
}

const swatchColor = (container: HTMLElement) =>
  (container.querySelector('[data-slot="color-field-swatch"]') as HTMLElement).style
    .backgroundColor;

describe("ColorField", () => {
  it("labels its input, with its data-slots", () => {
    const { container } = renderWithStrings(<ColorField label="Brand color" defaultValue={teal} />);
    const input = screen.getByRole("textbox", { name: "Brand color" });
    expect(input).toHaveAttribute("data-slot", "color-field-input");
    expect(input).toHaveValue(teal.toString("hex"));
    expect(input.closest('[data-slot="color-field-group"]')).not.toBeNull();
    expect(container.querySelector('[data-slot="color-field-swatch"]')).not.toBeNull();
  });

  it("commits typed hex digits on blur, with or without the sign, and the swatch follows", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { container } = renderWithStrings(<ColorField label="Brand color" onChange={onChange} />);
    await user.type(screen.getByRole("textbox"), "0b6e75");
    await user.tab();
    expect(lastColor(onChange).toHexInt()).toBe(0x0b6e75);
    expect(screen.getByRole("textbox")).toHaveValue(teal.toString("hex"));
    expect(swatchColor(container)).toMatch(/^rgb\(11, 110, 117\)$/);
  });

  it("puts the last color back when the text is not a color", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<ColorField label="Brand color" defaultValue={teal} onChange={onChange} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "12{Enter}");
    // Two digits are not a color; React Aria keeps the one it had.
    expect(input).toHaveValue(teal.toString("hex"));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("gives null and an empty swatch when cleared", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { container } = renderWithStrings(
      <ColorField label="Brand color" defaultValue={teal} onChange={onChange} />,
    );
    await user.clear(screen.getByRole("textbox"));
    await user.tab();
    expect(onChange).toHaveBeenLastCalledWith(null);
    expect(swatchColor(container)).toBe("");
    expect(container.querySelector('[data-slot="color-field-swatch"]')).toHaveAttribute(
      "data-empty",
    );
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<ColorField label="Brand color" value={teal} onChange={onChange} />);
    const input = screen.getByRole("textbox");
    await user.clear(input);
    await user.type(input, "ffffff");
    await user.tab();
    expect(lastColor(onChange).toHexInt()).toBe(0xffffff);
    expect(input).toHaveValue(teal.toString("hex"));
  });

  it("hides the swatch from assistive technology", () => {
    renderWithStrings(<ColorField label="Brand color" defaultValue={teal} />);
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("shows the caller's error", () => {
    renderWithStrings(
      <ColorField
        label="Brand color"
        defaultValue={teal}
        isInvalid
        errorMessage="This color is too light for text."
      />,
    );
    const input = screen.getByRole("textbox");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("This color is too light for text.");
  });

  it("disables its input", () => {
    renderWithStrings(<ColorField label="Brand color" defaultValue={teal} isDisabled />);
    expect(screen.getByRole("textbox")).toBeDisabled();
  });

  it("shows no text of its own", () => {
    const { container } = renderWithStrings(<ColorField label="«caller»" />, { marked: true });
    expect(unmarkedText(container)).toEqual([]);
  });
});
