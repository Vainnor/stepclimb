// SPDX-License-Identifier: AGPL-3.0-or-later
import { DotsThreeIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { IconButton } from "./icon-button";
import { Menu, MenuItem, MenuTrigger, SubmenuTrigger } from "./menu";

const meta = {
  title: "Actions/Menu",
  component: Menu,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A list of actions opened from a button. At 768 px and wider it opens in a popover beside the button; under 768 px it opens as a sheet from the bottom of the screen, where it is easy to reach with a thumb. A submenu always opens in a popover: beside its item, or above it under 768 px, where the item spans the sheet. Arrow keys move between items, typing jumps to an item, Escape closes the menu and returns focus to the button.

**Usage.** \`<MenuTrigger><Button>Course actions</Button><Menu onAction={run}><MenuItem id="edit">Edit</MenuItem></Menu></MenuTrigger>\`. \`selectionMode\` on \`Menu\` makes its items checkable; \`SubmenuTrigger\` wraps an item and the \`Menu\` it opens. \`isOpen\` and \`onOpenChange\` on \`MenuTrigger\` control it.

**Do** name items as actions, put the destructive one last, and keep submenus rare: on a phone they open over the sheet.

**Don't** use a menu for navigation between pages (that is a link), or for choosing a form value (that is a \`Select\`).`,
      },
    },
  },
} satisfies Meta<typeof Menu>;

export default meta;
type Story = StoryObj<typeof meta>;

const items = (
  <>
    <MenuItem id="edit">Edit</MenuItem>
    <MenuItem id="duplicate">Duplicate</MenuItem>
    <MenuItem id="archive" isDisabled>
      Archive
    </MenuItem>
    <MenuItem id="export">Export</MenuItem>
  </>
);

/** Closed: the trigger. */
export const Default: Story = {
  render: () => (
    <MenuTrigger>
      <Button>Course actions</Button>
      <Menu>{items}</Menu>
    </MenuTrigger>
  ),
};

/** Open: a popover at 1280 px, a bottom sheet at 375 px. The story fills the viewport so the
 * screenshot holds the sheet. */
export const Open: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <MenuTrigger defaultOpen>
        <Button>Course actions</Button>
        <Menu>{items}</Menu>
      </MenuTrigger>
    </div>
  ),
};

/** Checkable items: a check marks the selected one. */
export const Selection: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <MenuTrigger defaultOpen>
        <Button>Sort</Button>
        <Menu selectionMode="single" defaultSelectedKeys={["due"]}>
          <MenuItem id="due">Due date</MenuItem>
          <MenuItem id="name">Name</MenuItem>
          <MenuItem id="progress">Progress</MenuItem>
        </Menu>
      </MenuTrigger>
    </div>
  ),
};

/** An item that opens a submenu ends in a caret. */
export const Submenu: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <MenuTrigger defaultOpen>
        <IconButton icon={DotsThreeIcon} label="More actions" intent="secondary" />
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
      </MenuTrigger>
    </div>
  ),
};

/** The submenu open: beside its item at 1280 px, above it over the bottom sheet at 375 px. */
export const SubmenuOpen: Story = {
  render: Submenu.render,
  play: async () => {
    const body = within(document.body);
    await userEvent.click(await body.findByRole("menuitem", { name: "Export" }));
    const item = await body.findByRole("menuitem", { name: "Course archive" });
    await waitFor(() => expect(item).toBeVisible());
  },
};

export const LongContent: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <MenuTrigger defaultOpen>
        <Button>Roster actions</Button>
        <Menu>
          <MenuItem id="remind">
            Send a reminder to everyone who has not finished Ground control and Local control
          </MenuItem>
          <MenuItem id="export">Export the roster</MenuItem>
        </Menu>
      </MenuTrigger>
    </div>
  ),
};

/** The triggers in each brand, theme and density; the open menu is in the Open story's themes. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex items-center gap-3">
        <MenuTrigger>
          <Button>Course actions</Button>
          <Menu>{items}</Menu>
        </MenuTrigger>
        <MenuTrigger>
          <IconButton icon={DotsThreeIcon} label="More actions" intent="secondary" />
          <Menu>{items}</Menu>
        </MenuTrigger>
      </div>
    </ThemeMatrix>
  ),
};
