// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { learningStates, StatusChip } from "./status-chip";

const meta = {
  title: "Data/StatusChip",
  component: StatusChip,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `One learning state as a chip: locked, available, in progress, complete, overdue or needs review. Each state has one icon, weight and color everywhere in Stepclimb, and the icon's shape and the label tell the states apart without color.

**Usage.** \`<StatusChip state="complete" />\`. In a row whose text already says the state, \`labelHidden\` keeps the label for screen readers only.

**Do** use the state the server reports for the learner; one chip per item or module.

**Don't** use it for grades or feedback (those use the status colors), or recolor it with \`className\`.`,
      },
    },
  },
  args: { state: "available" },
} satisfies Meta<typeof StatusChip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const AllStates: Story = {
  render: () => (
    <div className="flex flex-wrap gap-2">
      {learningStates.map((state) => (
        <StatusChip key={state} state={state} />
      ))}
    </div>
  ),
};

export const LabelHidden: Story = {
  render: () => (
    <div className="flex flex-wrap gap-2">
      {learningStates.map((state) => (
        <StatusChip key={state} state={state} labelHidden />
      ))}
    </div>
  ),
};

/** In a narrow column the label truncates; the icon stays. */
export const LongContent: Story = {
  render: () => (
    <div className="w-24">
      <StatusChip state="review" />
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-wrap gap-2">
        {learningStates.map((state) => (
          <StatusChip key={state} state={state} />
        ))}
      </div>
    </ThemeMatrix>
  ),
};
