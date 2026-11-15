// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Code } from "./code";
import { Text } from "./text";

const meta = {
  title: "Foundations/Code",
  component: Code,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A short machine value inside running text: an error code, an ID, a version. Mono face, left to right in every direction, and it breaks anywhere rather than widen its column.

**Usage.** \`<Code>{problem.code}</Code>\`.

**Do** use it for values a person may copy or report, such as the code of an error the app has no message for.

**Don't** use it for keyboard keys (that is \`Kbd\`), for blocks of code (that is \`Prose\`), or to make ordinary text look technical.`,
      },
    },
  },
  args: { children: "auth.flow_expired" },
} satisfies Meta<typeof Code>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const InText: Story = {
  render: () => (
    <Text elementType="p">
      If it keeps happening, give an administrator this code: <Code>auth.flow_expired</Code>
    </Text>
  ),
};

export const LongContent: Story = {
  render: () => (
    <div className="max-w-48">
      <Text elementType="p">
        <Code>01900000-0000-7000-8000-000000000002.organization.membership_conflict</Code>
      </Text>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Text elementType="p">
        Error code <Code>auth.flow_expired</Code>
      </Text>
    </ThemeMatrix>
  ),
};
