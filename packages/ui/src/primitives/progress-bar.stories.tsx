// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ProgressBar } from "./progress-bar";

const meta = {
  title: "Data/ProgressBar",
  component: ProgressBar,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `How far something has got, with its label and its value as a percentage, from the strings catalog. Drawn in the brand color, which marks progress.

**Usage.** \`<ProgressBar label={t("course.progress")} value={3} maxValue={7} />\`. \`valueLabel\` replaces the percentage with words ("3 of 7 modules").

**Do** label it with what is progressing, and give the count when it means more than a percentage.

**Don't** use it for a quantity that is not progress, such as storage used (that is a \`Meter\`), or for a wait of unknown length (that is a pending \`Button\` or a \`Skeleton\`).`,
      },
    },
  },
  args: { label: "Ground control", value: 40 },
} satisfies Meta<typeof ProgressBar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = { args: { value: 0 } };

export const Complete: Story = { args: { value: 100 } };

/** A count instead of the percentage. */
export const ValueLabel: Story = {
  args: { label: "Modules", value: 3, maxValue: 7, valueLabel: "3 of 7" },
};

export const LongContent: Story = {
  args: {
    label: "Clearance delivery, ground control and local control for the spring cohort",
    value: 62,
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ProgressBar label="Ground control" value={40} />
      <ProgressBar label="Modules" value={3} maxValue={7} valueLabel="3 of 7" />
    </ThemeMatrix>
  ),
};
