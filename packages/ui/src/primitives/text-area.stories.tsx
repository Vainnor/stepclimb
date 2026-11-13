// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { TextArea } from "./text-area";

const meta = {
  title: "Forms/TextArea",
  component: TextArea,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled multi-line text input with help text and an error message, four lines tall by default and resizable in height.

**Usage.** \`<TextArea label={t("grading.feedback")} rows={6} />\`. It takes the same props as \`TextField\`.

**Do** size it with \`rows\` to the length you expect, and say in the description who will read it.

**Don't** use it for rich text (that is the editor) or for a one-line value.`,
      },
    },
  },
  args: { label: "Feedback", className: "max-w-md" },
} satisfies Meta<typeof TextArea>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { description: "The learner sees this with their grade." } };

export const Invalid: Story = {
  args: { isInvalid: true, errorMessage: "Write at least one sentence of feedback." },
};

export const Disabled: Story = {
  args: { isDisabled: true, defaultValue: "Good readbacks; watch the hold-short instructions." },
};

export const LongContent: Story = {
  args: {
    defaultValue:
      "Good readbacks throughout. On the second departure you cleared the aircraft for takeoff before the arrival had cleared the runway; check the runway is clear before you issue the clearance. Your phraseology for the hold-short instruction was correct.\n\nNext session: practice sequencing two arrivals with a departure in between.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <TextArea label="Feedback" description="The learner sees this with their grade." rows={2} />
      <TextArea
        label="Feedback"
        rows={2}
        isInvalid
        errorMessage="Write at least one sentence of feedback."
      />
    </ThemeMatrix>
  ),
};
