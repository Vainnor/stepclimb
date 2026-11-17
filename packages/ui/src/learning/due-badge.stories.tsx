// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { DueBadge } from "./due-badge";

const now = new Date("2026-03-10T14:00:00Z");
const day = 24 * 60 * 60 * 1000;
const offsets = [3, 1, 0.2, -0.05, -1, -3];

const meta = {
  title: "Learning/DueBadge",
  component: DueBadge,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `When something is due, in calendar days in the user's time zone: "Due in 3 days", "Due tomorrow", "Due today", and once the due time has passed, "Overdue" (earlier today), "1 day overdue" or "3 days overdue" with the overdue icon.

**Usage.** \`<DueBadge due={new Date(assignment.due_at)} now={clock.now()} timeZone={me.tz} />\`.

**Do** pass the user's time zone, so the day changes at their midnight.

**Don't** show it for work that is already complete, or recolor it with \`className\`.`,
      },
    },
  },
  args: { due: new Date(now.getTime() + 3 * day), now, timeZone: "America/New_York" },
} satisfies Meta<typeof DueBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Overdue: Story = { args: { due: new Date(now.getTime() - 3 * day) } };

export const AllStates: Story = {
  render: (args) => (
    <div className="flex flex-wrap gap-2">
      {offsets.map((offset) => (
        <DueBadge key={offset} {...args} due={new Date(now.getTime() + offset * day)} />
      ))}
    </div>
  ),
};

/** In a narrow column the label truncates; the icon stays. */
export const LongContent: Story = {
  render: (args) => (
    <div className="w-24">
      <DueBadge {...args} due={new Date(now.getTime() - 12 * day)} />
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <div className="flex flex-wrap gap-2">
        {offsets.map((offset) => (
          <DueBadge key={offset} {...args} due={new Date(now.getTime() + offset * day)} />
        ))}
      </div>
    </ThemeMatrix>
  ),
};
