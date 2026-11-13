// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Button } from "./button";
import { ButtonGroup } from "./button-group";

describe("ButtonGroup", () => {
  it("is a named group with its data-slot around its buttons", () => {
    render(
      <ButtonGroup aria-label="Attempt actions" className="ms-auto">
        <Button>Review</Button>
        <Button>Retake</Button>
      </ButtonGroup>,
    );
    const group = screen.getByRole("group", { name: "Attempt actions" });
    expect(group).toHaveAttribute("data-slot", "button-group");
    expect(group).toHaveClass("inline-flex", "gap-2", "ms-auto");
    expect(group).toContainElement(screen.getByRole("button", { name: "Retake" }));
  });

  it("leaves each button its own tab stop", async () => {
    const user = userEvent.setup();
    render(
      <ButtonGroup>
        <Button>Review</Button>
        <Button>Retake</Button>
      </ButtonGroup>,
    );
    await user.tab();
    expect(screen.getByRole("button", { name: "Review" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "Retake" })).toHaveFocus();
  });
});
