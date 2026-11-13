// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { TextField } from "./text-field";

describe("TextField", () => {
  it("labels its input and describes it, with its data-slots", () => {
    render(
      <TextField label="Display name" description="Shown to your instructors." placeholder="Ada" />,
    );
    const input = screen.getByRole("textbox", { name: "Display name" });
    expect(input).toHaveAttribute("data-slot", "text-field-input");
    expect(input).toHaveAccessibleDescription("Shown to your instructors.");
    expect(input).toHaveAttribute("placeholder", "Ada");
    const root = input.closest('[data-slot="text-field"]');
    expect(root?.querySelector('[data-slot="field-label"]')).toHaveTextContent("Display name");
    expect(root?.querySelector('[data-slot="field-description"]')).toBeInTheDocument();
    expect(root?.querySelector('[data-slot="field-error"]')).toBeNull();
  });

  it("works uncontrolled", async () => {
    const user = userEvent.setup();
    render(<TextField label="Display name" defaultValue="Ada" />);
    const input = screen.getByRole("textbox");
    await user.type(input, " L");
    expect(input).toHaveValue("Ada L");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<TextField label="Display name" value="Ada" onChange={onChange} />);
    const input = screen.getByRole("textbox");
    await user.type(input, "x");
    expect(onChange).toHaveBeenCalledWith("Adax");
    expect(input).toHaveValue("Ada");
  });

  it("shows its error message, after an icon, and marks the input invalid", () => {
    render(
      <TextField
        label="Email"
        isInvalid
        errorMessage="Enter an email address with an @, such as name@example.org."
      />,
    );
    const input = screen.getByRole("textbox", { name: "Email" });
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription(
      "Enter an email address with an @, such as name@example.org.",
    );
    const error = input
      .closest('[data-slot="text-field"]')
      ?.querySelector('[data-slot="field-error"]');
    expect(error?.querySelector('[data-slot="icon"]')).toHaveAttribute("aria-hidden", "true");
  });

  it("shows a validate function's message after a commit", async () => {
    const user = userEvent.setup();
    renderWithStrings(
      <TextField
        label="Code"
        validationBehavior="aria"
        validate={(v) => (v.length === 4 ? null : "Enter the four-character code from the email.")}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Code" });
    await user.type(input, "AB");
    await user.tab();
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Enter the four-character code from the email.")).toBeInTheDocument();
  });

  it("is disabled", () => {
    render(<TextField label="Display name" isDisabled />);
    expect(screen.getByRole("textbox")).toBeDisabled();
  });

  it("lets className add layout to the root", () => {
    render(<TextField label="Display name" className="max-w-sm" />);
    expect(screen.getByRole("textbox").closest('[data-slot="text-field"]')).toHaveClass(
      "max-w-sm",
      "flex-col",
    );
  });
});
