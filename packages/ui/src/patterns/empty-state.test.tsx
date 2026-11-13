// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon } from "@phosphor-icons/react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "../primitives/button";
import { EmptyState } from "./empty-state";

describe("EmptyState", () => {
  it("says why it is empty and offers its one action, with its data-slots", () => {
    const { container } = render(
      <EmptyState icon={BookOpenIcon} action={<Button intent="primary">Create a course</Button>}>
        No courses yet. Create one to start building lessons.
      </EmptyState>,
    );
    const root = container.querySelector('[data-slot="empty-state"]');
    expect(root?.querySelector('[data-slot="empty-state-description"]')).toHaveTextContent(
      "No courses yet. Create one to start building lessons.",
    );
    expect(root?.querySelector('[data-slot="empty-state-icon"] svg')).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(root?.querySelector('[data-slot="empty-state-action"]')).toContainElement(
      screen.getByRole("button", { name: "Create a course" }),
    );
  });

  it("draws no icon or action slot when given none", () => {
    const { container } = render(<EmptyState>Nothing is due this week.</EmptyState>);
    expect(container.querySelector('[data-slot="empty-state-icon"]')).toBeNull();
    expect(container.querySelector('[data-slot="empty-state-action"]')).toBeNull();
  });
});
