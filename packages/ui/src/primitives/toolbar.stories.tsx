// SPDX-License-Identifier: AGPL-3.0-or-later
import {
  LinkSimpleIcon,
  ListBulletsIcon,
  TextBolderIcon,
  TextItalicIcon,
} from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Icon } from "../theme/icon";
import { Button } from "./button";
import { IconButton } from "./icon-button";
import { Separator } from "./separator";
import { ToggleButton } from "./toggle-button";
import { Toolbar } from "./toolbar";

const meta = {
  title: "Actions/Toolbar",
  component: Toolbar,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A row of controls that is one tab stop: Tab moves into it and out of it, and the arrow keys move between its items.

**Usage.** \`<Toolbar aria-label={t("editor.formatting")}>\` around \`Button\`, \`IconButton\` and \`ToggleButton\`s; a \`Separator\` with \`orientation="vertical"\` divides groups. \`orientation="vertical"\` stacks it, and the up and down arrows move.

**Do** name it, and group related controls with separators.

**Don't** put text fields in it (the arrow keys belong to the field), or use it for a page's main actions (that is a \`ButtonGroup\`).`,
      },
    },
  },
} satisfies Meta<typeof Toolbar>;

export default meta;
type Story = StoryObj<typeof meta>;

function Formatting() {
  return (
    <Toolbar aria-label="Formatting">
      <ToggleButton intent="ghost" aria-label="Bold" className="aspect-square px-0">
        <Icon icon={TextBolderIcon} size="md" />
      </ToggleButton>
      <ToggleButton
        intent="ghost"
        aria-label="Italic"
        className="aspect-square px-0"
        defaultSelected
      >
        <Icon icon={TextItalicIcon} size="md" />
      </ToggleButton>
      <Separator orientation="vertical" />
      <IconButton icon={ListBulletsIcon} label="Bulleted list" />
      <IconButton icon={LinkSimpleIcon} label="Insert link" />
      <Separator orientation="vertical" />
      <Button size="sm">Preview</Button>
    </Toolbar>
  );
}

export const Default: Story = { render: () => <Formatting /> };

/** Tab moves into the toolbar; the arrow keys move inside it. */
export const FocusVisible: Story = {
  render: () => (
    <div className="pt-10">
      <Formatting />
    </div>
  ),
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await userEvent.keyboard("{ArrowRight}{ArrowRight}");
    await expect(
      within(canvasElement).getByRole("button", { name: "Bulleted list" }),
    ).toHaveAttribute("data-focus-visible");
  },
};

export const Vertical: Story = {
  render: () => (
    <Toolbar aria-label="Lesson tools" orientation="vertical" className="w-fit">
      <IconButton icon={ListBulletsIcon} label="Outline" />
      <IconButton icon={LinkSimpleIcon} label="Copy link" />
    </Toolbar>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Formatting />
    </ThemeMatrix>
  ),
};
