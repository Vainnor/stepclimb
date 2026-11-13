// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Meter } from "./meter";

const meta = {
  title: "Data/Meter",
  component: Meter,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A quantity within a known range, such as storage used, with its label and its value as a percentage, from the strings catalog. Drawn in a neutral fill: the brand color marks progress, which a meter is not.

**Usage.** \`<Meter label={t("storage.used")} value={used} maxValue={quota} />\`. \`valueLabel\` replaces the percentage with words ("7.2 of 10 GB").

**Do** give the amount in words when the percentage alone does not say enough.

**Don't** use it for how far a task or course has got (that is a \`ProgressBar\`), or color it to mean good or bad.`,
      },
    },
  },
  args: { label: "Storage used", value: 72 },
} satisfies Meta<typeof Meter>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const ValueLabel: Story = {
  args: { value: 7.2, maxValue: 10, valueLabel: "7.2 of 10 GB" },
};

export const Full: Story = { args: { value: 100 } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Meter label="Storage used" value={72} />
      <Meter label="Seats" value={18} maxValue={25} valueLabel="18 of 25" />
    </ThemeMatrix>
  ),
};
