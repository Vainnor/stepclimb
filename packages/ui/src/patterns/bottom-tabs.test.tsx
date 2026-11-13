// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, HouseIcon, UserIcon } from "@phosphor-icons/react";
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings } from "../test/strings";
import { BottomTabs } from "./bottom-tabs";
import type { NavEntry } from "./side-nav";

const entries: NavEntry[] = [
  { id: "home", href: "/", label: "Home", icon: HouseIcon },
  { id: "courses", href: "/courses", label: "Courses", icon: BookOpenIcon },
  { id: "account", href: "/me", label: "Account", icon: UserIcon },
];

describe("BottomTabs", () => {
  it("is a nav of links, not ARIA tabs, marking the current one with aria-current (T12)", () => {
    renderWithStrings(<BottomTabs entries={entries} current="home" />);
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(nav).toHaveAttribute("data-slot", "bottom-tabs");
    expect(within(nav).queryByRole("tab")).toBeNull();
    const links = within(nav).getAllByRole("link");
    expect(links.map((l) => l.textContent)).toEqual(["Home", "Courses", "Account"]);
    expect(links.map((l) => l.getAttribute("aria-current"))).toEqual(["page", null, null]);
    expect(links[0]).toHaveAttribute("data-slot", "bottom-tabs-link");
  });

  it("is fixed to the bottom inside the safe area and does not rubber-band", () => {
    renderWithStrings(<BottomTabs entries={entries} />);
    expect(screen.getByRole("navigation")).toHaveClass(
      "fixed",
      "bottom-0",
      "pb-[env(safe-area-inset-bottom)]",
      "overscroll-none",
    );
  });
});
