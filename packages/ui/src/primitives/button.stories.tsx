// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";

const meta = {
  title: "Primitives/Button",
  component: Button,
  tags: ["experimental"],
  parameters: {
    status: {
      missing: ["States", "Themes", "Accessibility", "Responsive and touch", "Tests", "Docs"],
    },
  },
  args: { children: "Start quiz" },
  argTypes: {
    intent: { control: "inline-radio", options: ["primary", "secondary", "ghost", "danger"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
  },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = { args: { intent: "primary", children: "Start quiz" } };
export const Secondary: Story = { args: { intent: "secondary", children: "Preview" } };
export const Ghost: Story = { args: { intent: "ghost", children: "Skip" } };
export const Danger: Story = { args: { intent: "danger", children: "Delete attempt" } };
export const Small: Story = { args: { size: "sm", children: "Small" } };
export const Disabled: Story = { args: { isDisabled: true, children: "Disabled" } };

/** Every intent and size in each brand, theme and density, side by side. */
export const Matrix: Story = {
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-wrap items-center gap-3">
        <Button intent="primary">Start quiz</Button>
        <Button>Preview</Button>
        <Button intent="ghost">Skip</Button>
        <Button intent="danger">Delete attempt</Button>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Button size="sm">Small</Button>
        <Button isDisabled>Disabled</Button>
        <Button size="lg" intent="primary">
          Large
        </Button>
      </div>
    </ThemeMatrix>
  ),
};
