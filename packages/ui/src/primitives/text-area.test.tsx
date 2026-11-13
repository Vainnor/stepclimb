// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TextArea } from "./text-area";

describe("TextArea", () => {
  it("is a labeled multi-line input with its data-slots and four rows", () => {
    render(<TextArea label="Feedback" description="The learner sees this." />);
    const input = screen.getByRole("textbox", { name: "Feedback" });
    expect(input.tagName).toBe("TEXTAREA");
    expect(input).toHaveAttribute("data-slot", "text-area-input");
    expect(input).toHaveAttribute("rows", "4");
    expect(input).toHaveAccessibleDescription("The learner sees this.");
    expect(input.closest('[data-slot="text-area"]')).not.toBeNull();
  });

  it("keeps new lines, uncontrolled", async () => {
    const user = userEvent.setup();
    render(<TextArea label="Feedback" />);
    const input = screen.getByRole("textbox");
    await user.type(input, "Good{Enter}phraseology");
    expect(input).toHaveValue("Good\nphraseology");
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<TextArea label="Feedback" value="" onChange={onChange} />);
    await user.type(screen.getByRole("textbox"), "a");
    expect(onChange).toHaveBeenCalledWith("a");
    expect(screen.getByRole("textbox")).toHaveValue("");
  });

  it("shows its error message when invalid", () => {
    render(<TextArea label="Feedback" isInvalid errorMessage="Write at least one sentence." />);
    expect(screen.getByRole("textbox")).toHaveAccessibleDescription("Write at least one sentence.");
  });
});
