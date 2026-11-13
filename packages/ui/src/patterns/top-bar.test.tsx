// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TopBar } from "./top-bar";

describe("TopBar", () => {
  it("is a header with the view's title as its h1 and actions at the inline end", () => {
    render(<TopBar title="S1 Ground Theory" actions={<button type="button">Account</button>} />);
    const header = screen.getByRole("banner");
    expect(header).toHaveAttribute("data-slot", "top-bar");
    expect(screen.getByRole("heading", { level: 1, name: "S1 Ground Theory" })).toHaveAttribute(
      "data-slot",
      "top-bar-title",
    );
    expect(header.querySelector('[data-slot="top-bar-actions"]')).toContainElement(
      screen.getByRole("button", { name: "Account" }),
    );
  });

  it("keeps clear of the safe areas at every width", () => {
    render(<TopBar title="Grades" />);
    const header = screen.getByRole("banner");
    expect(header).toHaveClass(
      "pt-[env(safe-area-inset-top)]",
      "md:ps-[max(1.5rem,env(safe-area-inset-left))]",
      "md:pe-[max(1.5rem,env(safe-area-inset-right))]",
      "rtl:md:ps-[max(1.5rem,env(safe-area-inset-right))]",
    );
    expect(header).not.toHaveClass("md:pt-0");
  });

  it("takes another heading level, and sticks inside the top safe area on phones", () => {
    render(<TopBar title="Grades" titleLevel={2} />);
    expect(screen.getByRole("heading", { level: 2, name: "Grades" })).toBeInTheDocument();
    expect(screen.getByRole("banner")).toHaveClass(
      "max-md:sticky",
      "max-md:backdrop-blur-md",
      "pt-[env(safe-area-inset-top)]",
    );
  });
});
