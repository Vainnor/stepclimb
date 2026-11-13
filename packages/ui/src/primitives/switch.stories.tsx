// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Switch } from "./switch";

const meta = {
  title: "Forms/Switch",
  component: Switch,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Turns a setting on or off at once, with no save step. The thumb moves and the track fills, so color is not the only sign. The label is the press target, at least 44 px tall on touch screens.

**Usage.** \`<Switch isSelected={on} onChange={setOn}>Email digest</Switch>\`, or \`defaultSelected\`.

**Do** label it with the setting ("Email digest"), and apply the change as soon as it flips.

**Don't** use it in a form that is saved with a button (that is a \`Checkbox\`), or label it "On" or "Off".`,
      },
    },
  },
  args: { children: "Email digest" },
} satisfies Meta<typeof Switch>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { description: "A weekly summary of your courses." } };

export const On: Story = { args: { defaultSelected: true } };

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("switch")).toHaveFocus();
  },
};

export const Disabled: Story = { args: { isDisabled: true, defaultSelected: true } };

export const LongContent: Story = {
  args: {
    children: "Remind me about deadlines in every course I am enrolled in, including archived ones",
    description: "Reminders go out three days and one day before each deadline.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col">
        <Switch>Email digest</Switch>
        <Switch defaultSelected>Reminders</Switch>
        <Switch isDisabled>Locked setting</Switch>
      </div>
    </ThemeMatrix>
  ),
};
