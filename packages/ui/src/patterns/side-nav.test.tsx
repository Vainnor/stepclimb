// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, HouseIcon, UserIcon } from "@phosphor-icons/react";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SIDE_NAV_EXPANDED_QUERY } from "../lib/breakpoints";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { type NavEntry, SideNav } from "./side-nav";

const entries: NavEntry[] = [
  { id: "home", href: "/", label: "Home", icon: HouseIcon },
  { id: "courses", href: "/courses", label: "Courses", icon: BookOpenIcon },
  { id: "account", href: "/me", label: "Account", icon: UserIcon },
];

function width(expanded: boolean) {
  stubMatchMedia((query) => (query === SIDE_NAV_EXPANDED_QUERY ? expanded : false));
}

describe("SideNav", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is a named nav that marks only the current entry with aria-current (T12)", () => {
    width(true);
    renderWithStrings(
      <SideNav entries={entries} current="courses" logo="Example Org" footer="Back" />,
    );
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(nav).toHaveAttribute("data-slot", "side-nav");
    const links = within(nav).getAllByRole("link");
    expect(links.map((l) => l.getAttribute("href"))).toEqual(["/", "/courses", "/me"]);
    expect(links.map((l) => l.getAttribute("aria-current"))).toEqual([null, "page", null]);
    expect(links[1]).toHaveAttribute("data-slot", "side-nav-link");
    expect(nav.querySelector('[data-slot="side-nav-logo"]')).toHaveTextContent("Example Org");
    expect(nav.querySelector('[data-slot="side-nav-footer"]')).toHaveTextContent("Back");
  });

  it("shows labels at 1024 px and wider and collapses to icons under it (spec 11 Q1)", () => {
    width(true);
    const { unmount } = renderWithStrings(<SideNav entries={entries} />);
    expect(screen.getByRole("navigation")).not.toHaveAttribute("data-collapsed");
    expect(screen.getByRole("navigation")).toHaveClass("w-60");
    unmount();
    width(false);
    renderWithStrings(<SideNav entries={entries} />);
    const nav = screen.getByRole("navigation");
    expect(nav).toHaveAttribute("data-collapsed", "true");
    expect(nav).toHaveClass("w-16");
    // The label stays the link's name.
    expect(within(nav).getByRole("link", { name: "Courses" })).toBeInTheDocument();
  });

  it("follows the collapsed prop over the width", () => {
    width(true);
    renderWithStrings(<SideNav entries={entries} collapsed />);
    expect(screen.getByRole("navigation")).toHaveAttribute("data-collapsed", "true");
  });

  it("shows a collapsed entry's label as a tooltip on keyboard focus", async () => {
    width(false);
    const user = userEvent.setup();
    renderWithStrings(<SideNav entries={entries} />);
    await user.tab();
    expect(screen.getByRole("link", { name: "Home" })).toHaveFocus();
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveTextContent("Home");
    expect(tip).toHaveAttribute("data-slot", "side-nav-tooltip");
  });
});
