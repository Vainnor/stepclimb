// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { ButtonGroup } from "./button-group";

const meta = {
  title: "Actions/ButtonGroup",
  component: ButtonGroup,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Related buttons side by side, as one group for screen readers. Each button stays its own tab stop; the buttons wrap onto a new line when the row is too narrow.

**Usage.** \`<ButtonGroup aria-label={t("attempt.actions")}><Button>Review</Button><Button>Retake</Button></ButtonGroup>\`.

**Do** group the actions that belong to one thing, and name the group when the buttons need that context.

**Don't** put more than one primary button in a group, or use it for controls that should move with the arrow keys (that is a \`Toolbar\`).`,
      },
    },
  },
} satisfies Meta<typeof ButtonGroup>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: () => (
    <ButtonGroup aria-label="Attempt 2">
      <Button intent="primary">Review answers</Button>
      <Button>Retake quiz</Button>
    </ButtonGroup>
  ),
};

/** At 375 px the buttons wrap instead of overflowing. */
export const LongContent: Story = {
  render: () => (
    <ButtonGroup aria-label="Lesson">
      <Button>Previous lesson: Clearance delivery</Button>
      <Button intent="primary">Next lesson: Ground control</Button>
    </ButtonGroup>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ButtonGroup aria-label="Attempt 2">
        <Button intent="primary">Review answers</Button>
        <Button>Retake quiz</Button>
        <Button intent="ghost">Skip</Button>
      </ButtonGroup>
    </ThemeMatrix>
  ),
};
