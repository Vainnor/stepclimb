// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, CalendarCheckIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../primitives/button";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { EmptyState } from "./empty-state";

const meta = {
  title: "Data/EmptyState",
  component: EmptyState,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `What an empty list or view shows: one sentence on why it is empty and, when the person can do something about it, one action.

**Usage.** \`<EmptyState icon={BookOpenIcon} action={<Button intent="primary">{t("course.create")}</Button>}>{t("courses.empty")}</EmptyState>\`.

**Do** say why it is empty in plain words ("No courses yet"), and offer the action only to someone who can take it.

**Don't** add a second action, a heading over the sentence, or an illustration; never show it while the list is still loading (that is a \`Skeleton\`).`,
      },
    },
  },
  args: { children: "Nothing is due this week." },
  argTypes: { icon: { control: false }, action: { control: false } },
} satisfies Meta<typeof EmptyState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { icon: CalendarCheckIcon } };

/** With its one action, for someone who can fill the list. */
export const WithAction: Story = {
  args: {
    icon: BookOpenIcon,
    children: "No courses yet. Create one to start writing lessons.",
    action: <Button intent="primary">Create a course</Button>,
  },
};

export const LongContent: Story = {
  args: {
    children:
      "No one in your team has an assignment due in the next 30 days, so there is nothing to follow up on.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <EmptyState icon={BookOpenIcon} action={<Button intent="primary">Create a course</Button>}>
        No courses yet. Create one to start writing lessons.
      </EmptyState>
    </ThemeMatrix>
  ),
};
