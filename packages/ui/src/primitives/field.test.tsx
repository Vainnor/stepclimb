// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { TextField as AriaTextField, Input } from "react-aria-components";
import { describe, expect, it } from "vitest";
import { FieldDescription, FieldError, FieldLabel, field } from "./field";

describe("Field parts", () => {
  it("wire a label, a description and an error to the input, with their data-slots", () => {
    render(
      <AriaTextField isInvalid>
        <FieldLabel>Callsign</FieldLabel>
        <Input />
        <FieldDescription>As filed, for example DAL123.</FieldDescription>
        <FieldError>Use letters and digits only, such as DAL123.</FieldError>
      </AriaTextField>,
    );
    const input = screen.getByRole("textbox", { name: "Callsign" });
    expect(input).toHaveAccessibleDescription(
      "As filed, for example DAL123. Use letters and digits only, such as DAL123.",
    );
    expect(screen.getByText("Callsign")).toHaveAttribute("data-slot", "field-label");
    expect(screen.getByText(/^As filed/)).toHaveAttribute("data-slot", "field-description");
    expect(screen.getByText(/^Use letters/).closest('[data-slot="field-error"]')).not.toBeNull();
  });

  it("show the validation errors when the error has no text of its own", () => {
    render(
      <AriaTextField validationBehavior="aria" validate={() => "Too short."}>
        <FieldLabel>Callsign</FieldLabel>
        <Input />
        <FieldError />
      </AriaTextField>,
    );
    expect(screen.getByText("Too short.")).toBeInTheDocument();
  });

  it("render no error while the field is valid", () => {
    const { container } = render(
      <AriaTextField>
        <FieldLabel>Callsign</FieldLabel>
        <Input />
        <FieldError>Use letters and digits only.</FieldError>
      </AriaTextField>,
    );
    expect(container.querySelector('[data-slot="field-error"]')).toBeNull();
  });

  it("draw the control from tokens: sunken well, strong line, danger when invalid", () => {
    const control = field().control();
    expect(control).toContain("bg-sunken");
    expect(control).toContain("border-line-strong");
    expect(control).toContain("data-[invalid]:border-danger");
    expect(field().input()).toContain("placeholder:text-fg-soft");
  });
});
