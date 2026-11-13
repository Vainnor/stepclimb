// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Breadcrumb, Breadcrumbs } from "./breadcrumbs";

describe("Breadcrumbs", () => {
  it("is a named nav whose last crumb is the current page, not a link", () => {
    renderWithStrings(
      <Breadcrumbs>
        <Breadcrumb href="/courses">Courses</Breadcrumb>
        <Breadcrumb href="/courses/s1">S1 Ground Theory</Breadcrumb>
        <Breadcrumb>Taxi instructions</Breadcrumb>
      </Breadcrumbs>,
    );
    const nav = screen.getByRole("navigation", { name: "Breadcrumbs" });
    expect(nav).toHaveAttribute("data-slot", "breadcrumbs");
    // Only the nav is named; React Aria's own name on the list is removed.
    expect(nav.querySelector("ol")).not.toHaveAttribute("aria-label");
    const links = within(nav).getAllByRole("link");
    expect(links.map((l) => l.textContent)).toEqual([
      "Courses",
      "S1 Ground Theory",
      "Taxi instructions",
    ]);
    expect(links[0]).toHaveAttribute("href", "/courses");
    expect(links[0]).toHaveAttribute("data-slot", "breadcrumb-link");
    expect(links[2]).toHaveAttribute("aria-current", "page");
    expect(links[2]).not.toHaveAttribute("href");
    expect(links[0]).not.toHaveAttribute("aria-current");
    // A caret between crumbs, none after the current page.
    expect(nav.querySelectorAll('[data-slot="breadcrumb"] [data-slot="icon"]')).toHaveLength(2);
    expect(nav.querySelectorAll('[data-slot="breadcrumb"]')).toHaveLength(3);
  });
});
