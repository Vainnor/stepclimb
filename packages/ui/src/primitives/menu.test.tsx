// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { ThemeProvider } from "../theme/theme-provider";
import { Button } from "./button";
import { MENU_POPOVER_QUERY, Menu, MenuItem, MenuTrigger, SubmenuTrigger } from "./menu";

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === MENU_POPOVER_QUERY ? isWide : false));
}

function Example(props: {
  isOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  onAction?: (key: React.Key) => void;
}) {
  const { onAction, ...trigger } = props;
  return (
    <MenuTrigger {...trigger}>
      <Button>Course actions</Button>
      <Menu onAction={onAction}>
        <MenuItem id="edit">Edit</MenuItem>
        <MenuItem id="duplicate">Duplicate</MenuItem>
        <MenuItem id="archive" isDisabled>
          Archive
        </MenuItem>
        <MenuItem id="export">Export</MenuItem>
      </Menu>
    </MenuTrigger>
  );
}

describe("Menu", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("opens in a Popover at 768 px and wider, with its data-slots", async () => {
    wide(true);
    const user = userEvent.setup();
    render(<Example />);
    await user.click(screen.getByRole("button", { name: "Course actions" }));
    const menu = screen.getByRole("menu", { name: "Course actions" });
    expect(menu).toHaveAttribute("data-slot", "menu");
    expect(menu.closest('[data-slot="menu-popover"]')).not.toBeNull();
    expect(menu.closest('[data-slot="menu-sheet"]')).toBeNull();
    const item = screen.getByRole("menuitem", { name: "Edit" });
    expect(item).toHaveAttribute("data-slot", "menu-item");
    expect(item.querySelector('[data-slot="menu-item-label"]')).toHaveTextContent("Edit");
  });

  it("opens as a bottom sheet under 768 px (Q10)", async () => {
    wide(false);
    const user = userEvent.setup();
    render(<Example />);
    await user.click(screen.getByRole("button", { name: "Course actions" }));
    const menu = screen.getByRole("menu", { name: "Course actions" });
    expect(menu.closest('[data-slot="menu-sheet"]')).not.toBeNull();
    expect(menu.closest('[data-slot="menu-sheet-overlay"]')).toHaveClass("bg-overlay");
    expect(menu.closest('[data-slot="menu-popover"]')).toBeNull();
  });

  it.each([
    ["Popover", true],
    ["sheet", false],
  ])(
    "moves with the arrow keys, skips disabled items and acts on Enter in a %s (T12)",
    async (_, isWide) => {
      wide(isWide);
      const user = userEvent.setup();
      const onAction = vi.fn();
      render(<Example onAction={onAction} />);
      screen.getByRole("button", { name: "Course actions" }).focus();
      await user.keyboard("{Enter}");
      expect(screen.getByRole("menuitem", { name: "Edit" })).toHaveFocus();
      await user.keyboard("{ArrowDown}");
      expect(screen.getByRole("menuitem", { name: "Duplicate" })).toHaveFocus();
      await user.keyboard("{ArrowDown}");
      expect(screen.getByRole("menuitem", { name: "Export" })).toHaveFocus();
      await user.keyboard("{ArrowUp}");
      expect(screen.getByRole("menuitem", { name: "Duplicate" })).toHaveFocus();
      await user.keyboard("{Enter}");
      expect(onAction).toHaveBeenCalledOnce();
      expect(onAction.mock.calls[0]?.[0]).toBe("duplicate");
      expect(screen.queryByRole("menu")).toBeNull();
    },
  );

  it.each([
    ["Popover", true],
    ["sheet", false],
  ])("closes on Escape and returns focus to the trigger in a %s", async (_, isWide) => {
    wide(isWide);
    const user = userEvent.setup();
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Course actions" });
    await user.click(trigger);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("jumps to an item by typing its first letters", async () => {
    wide(true);
    const user = userEvent.setup();
    render(<Example />);
    screen.getByRole("button", { name: "Course actions" }).focus();
    await user.keyboard("{Enter}");
    await user.keyboard("ex");
    expect(screen.getByRole("menuitem", { name: "Export" })).toHaveFocus();
  });

  it("works controlled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const { rerender } = render(<Example isOpen={false} onOpenChange={onOpenChange} />);
    await user.click(screen.getByRole("button", { name: "Course actions" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("menu")).toBeNull();
    rerender(<Example isOpen onOpenChange={onOpenChange} />);
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("marks selected items with a check, controlled and uncontrolled", async () => {
    wide(true);
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(
      <MenuTrigger defaultOpen>
        <Button>Sort</Button>
        <Menu
          selectionMode="single"
          defaultSelectedKeys={["due"]}
          onSelectionChange={onSelectionChange}
        >
          <MenuItem id="due">Due date</MenuItem>
          <MenuItem id="name">Name</MenuItem>
        </Menu>
      </MenuTrigger>,
    );
    const due = screen.getByRole("menuitemradio", { name: "Due date" });
    expect(due).toHaveAttribute("aria-checked", "true");
    expect(due.querySelector('[data-slot="menu-item-check"] [data-slot="icon"]')).not.toBeNull();
    const name = screen.getByRole("menuitemradio", { name: "Name" });
    expect(name.querySelector('[data-slot="menu-item-check"] [data-slot="icon"]')).toBeNull();
    await user.click(name);
    expect([...(onSelectionChange.mock.calls[0]?.[0] ?? [])]).toEqual(["name"]);
  });

  it("opens a submenu in a Popover beside its item, even when the menu is a sheet", async () => {
    wide(false);
    const user = userEvent.setup();
    render(
      <MenuTrigger>
        <Button>Course actions</Button>
        <Menu>
          <MenuItem id="edit">Edit</MenuItem>
          <SubmenuTrigger>
            <MenuItem id="export">Export</MenuItem>
            <Menu>
              <MenuItem id="archive">Course archive</MenuItem>
              <MenuItem id="csv">Grades as CSV</MenuItem>
            </Menu>
          </SubmenuTrigger>
        </Menu>
      </MenuTrigger>,
    );
    await user.click(screen.getByRole("button", { name: "Course actions" }));
    const exportItem = screen.getByRole("menuitem", { name: "Export" });
    expect(exportItem).toHaveAttribute("aria-haspopup", "menu");
    expect(exportItem.querySelector('[data-slot="icon"]')).not.toBeNull();
    exportItem.focus();
    await user.keyboard("{ArrowRight}");
    const submenu = await screen.findByRole("menu", { name: "Export" });
    const submenuPopover = submenu.closest('[data-slot="menu-popover"]');
    expect(submenuPopover).toHaveClass("min-w-48");
    expect(submenuPopover).not.toHaveClass("min-w-(--trigger-width)");
    expect(within(submenu).getByRole("menuitem", { name: "Course archive" })).toHaveFocus();
  });

  it("draws the sheet and the Popover in the surrounding theme", async () => {
    const user = userEvent.setup();
    for (const isWide of [true, false]) {
      wide(isWide);
      const { unmount } = render(
        <ThemeProvider theme="dark" brand="vzdc">
          <Example />
        </ThemeProvider>,
      );
      await user.click(screen.getByRole("button", { name: "Course actions" }));
      const slot = isWide ? "menu-popover" : "menu-sheet-overlay";
      const overlay = screen.getByRole("menu").closest(`[data-slot="${slot}"]`);
      expect(overlay).toHaveAttribute("data-sc-theme", "dark");
      expect(overlay).toHaveAttribute("data-sc-brand", "vzdc");
      unmount();
    }
  });
});
