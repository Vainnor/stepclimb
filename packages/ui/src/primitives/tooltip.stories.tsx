// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { Tooltip, TooltipTrigger } from "./tooltip";

const meta = {
  title: "Overlays/Tooltip",
  component: Tooltip,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A short label shown when its trigger is hovered or focused with the keyboard, and hidden on Escape. Screen readers hear it as the trigger's description.

**Usage.** \`<TooltipTrigger><Button>Due 14 Nov</Button><Tooltip>{fullDate}</Tooltip></TooltipTrigger>\`. \`IconButton\` already shows its label this way.

**Do** keep it to a few words that repeat or expand what is on the trigger.

**Don't** make it the only place information lives: touch screens never show it. Never put a link or a button in it.`,
      },
    },
  },
} satisfies Meta<typeof Tooltip>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Shown on keyboard focus. */
export const Default: Story = {
  render: () => (
    <div className="pt-10">
      <TooltipTrigger>
        <Button>Due 14 Nov</Button>
        <Tooltip>Thursday, 14 November, 23:59 UTC</Tooltip>
      </TooltipTrigger>
    </div>
  ),
  play: async () => {
    await userEvent.tab();
    const tooltip = await within(document.body).findByRole("tooltip");
    await waitFor(() => expect(tooltip).toBeVisible());
  },
};

/** Wraps at its maximum width. It still only expands what the trigger says. */
export const LongContent: Story = {
  render: () => (
    <div className="pt-16">
      <TooltipTrigger defaultOpen>
        <Button>Due 14 Nov</Button>
        <Tooltip>Thursday, 14 November 2026, at 23:59 Coordinated Universal Time</Tooltip>
      </TooltipTrigger>
    </div>
  ),
};

/** Open in every brand, theme and density: each draws in its own cell's theme. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="pb-8">
        <TooltipTrigger isOpen>
          <Button>Due 14 Nov</Button>
          <Tooltip placement="bottom">Thursday, 23:59 UTC</Tooltip>
        </TooltipTrigger>
      </div>
    </ThemeMatrix>
  ),
};
