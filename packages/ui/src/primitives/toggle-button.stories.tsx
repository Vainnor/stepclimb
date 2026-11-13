// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ToggleButton, ToggleButtonGroup } from "./toggle-button";

const meta = {
  title: "Actions/ToggleButton",
  component: ToggleButton,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A button that stays pressed until pressed again, and \`ToggleButtonGroup\`, a set of them with single or multiple selection. Selected is drawn in the brand tint; screen readers hear "pressed", or "selected" for a radio in a single-selection group.

**Usage.** \`<ToggleButton defaultSelected>Show answers</ToggleButton>\`, or \`<ToggleButtonGroup aria-label={t("view")} defaultSelectedKeys={["list"]}>\` around toggle buttons with \`id\`s. Arrow keys move inside a group.

**Do** use it for an on/off setting that applies at once, such as a view or a format, with text that names the setting.

**Don't** use it for an action (that is a \`Button\`) or for a setting saved with a form (that is a \`Switch\` or \`Checkbox\`).`,
      },
    },
  },
  args: { children: "Show answers" },
  argTypes: {
    intent: { control: "inline-radio", options: ["secondary", "ghost"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
  },
} satisfies Meta<typeof ToggleButton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Selected: Story = { args: { defaultSelected: true } };

/** Single selection: one view at a time, like a segmented control. */
export const Group: Story = {
  render: () => (
    <ToggleButtonGroup aria-label="View" defaultSelectedKeys={["list"]}>
      <ToggleButton id="list">List</ToggleButton>
      <ToggleButton id="board">Board</ToggleButton>
      <ToggleButton id="calendar">Calendar</ToggleButton>
    </ToggleButtonGroup>
  ),
};

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("button")).toHaveAttribute("data-focus-visible");
  },
};

export const Disabled: Story = { args: { isDisabled: true, defaultSelected: true } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ToggleButtonGroup aria-label="View" defaultSelectedKeys={["list"]}>
        <ToggleButton id="list">List</ToggleButton>
        <ToggleButton id="board">Board</ToggleButton>
        <ToggleButton id="calendar" isDisabled>
          Calendar
        </ToggleButton>
      </ToggleButtonGroup>
      <div className="flex gap-3">
        <ToggleButton intent="ghost" defaultSelected>
          Show answers
        </ToggleButton>
        <ToggleButton intent="ghost">Show hints</ToggleButton>
      </div>
    </ThemeMatrix>
  ),
};
