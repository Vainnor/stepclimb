// SPDX-License-Identifier: AGPL-3.0-or-later
import { DotsThreeIcon, GearIcon, PencilSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { IconButton } from "./icon-button";

const meta = {
  title: "Actions/IconButton",
  component: IconButton,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A button whose only content is a Phosphor icon. Its \`label\` is required: it is the button's accessible name and shows as a tooltip on hover and keyboard focus.

**Usage.** \`<IconButton icon={GearIcon} label={t("course.settings")} />\`. Ghost by default, since icon buttons sit in toolbars and rows; \`intent\` and \`size\` work as on \`Button\`.

**Do** use it where the icon is familiar (edit, delete, more) and space is short, and take the label from the strings catalog.

**Don't** use it for the view's main action (that is a \`Button\` with text), or rely on the tooltip to explain an unfamiliar icon: touch screens never show it.`,
      },
    },
  },
  args: { icon: GearIcon, label: "Course settings" },
  argTypes: {
    icon: { control: false },
    intent: { control: "inline-radio", options: ["primary", "secondary", "ghost", "danger"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
  },
} satisfies Meta<typeof IconButton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Intents: Story = {
  render: () => (
    <div className="flex items-center gap-3">
      <IconButton icon={PencilSimpleIcon} label="Edit" />
      <IconButton icon={DotsThreeIcon} label="More actions" intent="secondary" />
      <IconButton icon={TrashIcon} label="Delete" intent="danger" />
    </div>
  ),
};

/** Keyboard focus draws the ring and shows the label as a tooltip. */
export const FocusVisible: Story = {
  render: (args) => (
    <div className="pt-10">
      <IconButton {...args} />
    </div>
  ),
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("button")).toHaveAttribute("data-focus-visible");
    await expect(await within(document.body).findByRole("tooltip")).toBeVisible();
  },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-wrap items-center gap-3">
        <IconButton icon={PencilSimpleIcon} label="Edit" />
        <IconButton icon={DotsThreeIcon} label="More actions" intent="secondary" />
        <IconButton icon={TrashIcon} label="Delete" intent="danger" />
        <IconButton icon={GearIcon} label="Settings" size="sm" />
        <IconButton icon={GearIcon} label="Settings" isDisabled />
      </div>
    </ThemeMatrix>
  ),
};
