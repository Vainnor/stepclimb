// SPDX-License-Identifier: AGPL-3.0-or-later
import { HouseIcon } from "@phosphor-icons/react";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { AppShell } from "./app-shell";
import { BottomTabs } from "./bottom-tabs";
import { type NavEntry, SideNav } from "./side-nav";
import { TopBar } from "./top-bar";

const entries: NavEntry[] = [{ id: "home", href: "/", label: "Home", icon: HouseIcon }];

function Example() {
  return (
    <AppShell
      sideNav={<SideNav entries={entries} current="home" />}
      bottomTabs={<BottomTabs entries={entries} current="home" />}
      topBar={<TopBar title="Home" />}
    >
      <button type="button">First control</button>
    </AppShell>
  );
}

describe("AppShell", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("has the header and main landmarks, with the navigation at each breakpoint", () => {
    stubMatchMedia(() => true);
    renderWithStrings(<Example />);
    expect(screen.getByRole("banner")).toBeInTheDocument();
    const main = screen.getByRole("main");
    expect(main).toHaveAttribute("id", "main");
    expect(main).toHaveClass(
      "pb-(--sc-bottom-tabs-inset)",
      "md:pb-0",
      "pe-[env(safe-area-inset-right)]",
      "rtl:pe-[env(safe-area-inset-left)]",
    );
    const side = document.querySelector('[data-slot="app-shell-side"]');
    const bottom = document.querySelector('[data-slot="app-shell-bottom"]');
    expect(side).toHaveClass("hidden", "md:block");
    expect(bottom).toHaveClass("md:hidden");
    expect(side?.querySelector('[data-slot="side-nav"]')).not.toBeNull();
    expect(bottom?.querySelector('[data-slot="bottom-tabs"]')).not.toBeNull();
  });

  it("leaves room for BottomTabs only when it has them", () => {
    stubMatchMedia(() => true);
    renderWithStrings(<AppShell topBar={<TopBar title="Home" />}>Content</AppShell>);
    expect(screen.getByRole("main")).not.toHaveClass("pb-(--sc-bottom-tabs-inset)");
  });

  it("starts with a skip link that moves focus to main (spec 11 B6)", async () => {
    stubMatchMedia(() => true);
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.tab();
    const skip = screen.getByRole("link", { name: "Skip to main content" });
    expect(skip).toHaveFocus();
    expect(skip).toHaveAttribute("href", "#main");
    await user.keyboard("{Enter}");
    expect(screen.getByRole("main")).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "First control" })).toHaveFocus();
  });
});
