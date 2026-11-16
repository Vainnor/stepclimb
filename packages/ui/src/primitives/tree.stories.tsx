// SPDX-License-Identifier: AGPL-3.0-or-later
import { FileIcon, FolderIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Tree, TreeItem } from "./tree";

const meta = {
  title: "Data/Tree",
  component: Tree,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A hierarchy whose rows open and close: the account tree, a course's file folders. The arrow keys move between rows, Right opens a row and Left closes it or moves to its parent, and typing jumps to a row. Rows indent by level and are 44 px tall on touch screens.

**Usage.** \`<Tree aria-label={t("files.folders")} selectionMode="single" selectedKeys={[folder]} onSelectionChange={…}>{folders.map(renderFolder)}</Tree>\`, where \`renderFolder\` returns \`<TreeItem id={f.path} title={f.name} icon={FolderIcon}>{f.children.map(renderFolder)}</TreeItem>\`. \`expandedKeys\` and \`onExpandedChange\` control which rows are open; \`defaultExpandedKeys\` leaves it uncontrolled.

**Do** give every row a stable \`id\`, and a \`textValue\` when its title is not plain text.

**Don't** use it for a flat list (a \`DataList\`) or for navigation menus (the \`SideNav\`).`,
      },
    },
  },
  args: { "aria-label": "Accounts", className: "max-w-sm" },
} satisfies Meta<typeof Tree>;

export default meta;
type Story = StoryObj<typeof meta>;

const accounts = (
  <TreeItem id="root" title="Example Air Traffic" icon={FolderIcon}>
    <TreeItem id="ground" title="Ground school" icon={FolderIcon}>
      <TreeItem id="delivery" title="Clearance delivery" icon={FolderIcon} />
      <TreeItem id="ground-control" title="Ground control" icon={FolderIcon} />
    </TreeItem>
    <TreeItem id="tower" title="Tower" icon={FolderIcon} />
    <TreeItem id="enroute" title="Enroute" icon={FolderIcon} isDisabled />
  </TreeItem>
);

export const Default: Story = {
  render: (args) => (
    <Tree {...args} defaultExpandedKeys={["root", "ground"]}>
      {accounts}
    </Tree>
  ),
};

export const Collapsed: Story = { render: (args) => <Tree {...args}>{accounts}</Tree> };

export const Selected: Story = {
  render: (args) => (
    <Tree
      {...args}
      aria-label="Folders"
      selectionMode="single"
      defaultSelectedKeys={["handouts"]}
      defaultExpandedKeys={["course"]}
    >
      <TreeItem id="course" title="Course files" icon={FolderIcon}>
        <TreeItem id="handouts" title="Handouts" icon={FolderIcon}>
          <TreeItem id="sop" title="phraseology.pdf" icon={FileIcon} />
        </TreeItem>
        <TreeItem id="images" title="Images" icon={FolderIcon} />
      </TreeItem>
    </Tree>
  ),
};

export const FocusVisible: Story = {
  render: (args) => (
    <Tree {...args} defaultExpandedKeys={["root"]}>
      {accounts}
    </Tree>
  ),
  // A Tab that runs as the story renders, before the rows are found in the page, leaves focus on
  // the tree and no row. So the play waits for the first row, tabs, and waits for React Aria to
  // move focus onto the row, which it does after the tree's own focus event.
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const first = await canvas.findByRole("row", { name: /^Example Air Traffic/ });
    await userEvent.tab();
    await waitFor(() => expect(first).toHaveFocus());
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(() => expect(canvas.getByRole("row", { name: /^Ground school/ })).toHaveFocus());
  },
};

export const Empty: Story = {
  render: (args) => (
    <Tree {...args} aria-label="Folders" emptyState="This course has no folders yet.">
      {[]}
    </Tree>
  ),
};

export const LongContent: Story = {
  render: (args) => (
    <Tree {...args} defaultExpandedKeys={["root", "a"]} className="max-w-64">
      <TreeItem
        id="root"
        title="Example Air Traffic Control Training Organization"
        icon={FolderIcon}
      >
        <TreeItem
          id="a"
          title="Approach and departure control for the terminal area"
          icon={FolderIcon}
        >
          <TreeItem
            id="b"
            title="Simultaneous independent approaches to parallel runways"
            icon={FolderIcon}
          />
        </TreeItem>
      </TreeItem>
    </Tree>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Tree
        aria-label="Accounts"
        selectionMode="single"
        defaultSelectedKeys={["tower"]}
        defaultExpandedKeys={["root"]}
      >
        {accounts}
      </Tree>
    </ThemeMatrix>
  ),
};
