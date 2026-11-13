// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "../primitives/button";
import { ErrorState } from "./error-state";

describe("ErrorState", () => {
  it("names what happened as a heading, says how to fix it, and holds the retry action", async () => {
    const user = userEvent.setup();
    const retry = vi.fn();
    const { container } = render(
      <ErrorState
        level={1}
        title="The course could not be loaded"
        action={<Button onPress={retry}>Try again</Button>}
      >
        Check your connection, then try again.
      </ErrorState>,
    );
    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("The course could not be loaded");
    expect(heading).toHaveAttribute("data-slot", "error-state-title");
    expect(container.querySelector('[data-slot="error-state-description"]')).toHaveTextContent(
      "Check your connection, then try again.",
    );
    expect(container.querySelector('[data-slot="error-state-icon"] svg')).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(retry).toHaveBeenCalledOnce();
  });

  it("draws no action slot when given none", () => {
    const { container } = render(
      <ErrorState level={2} title="Page not found">
        Check the address, or go back to the home page.
      </ErrorState>,
    );
    expect(screen.getByRole("heading", { level: 2 })).toBeInTheDocument();
    expect(container.querySelector('[data-slot="error-state-action"]')).toBeNull();
  });
});
