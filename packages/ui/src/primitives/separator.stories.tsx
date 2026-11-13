// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Separator } from "./separator";
import { Text } from "./text";

const meta = {
  title: "Foundations/Separator",
  component: Separator,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A rule between groups of content, in \`line\` (dividers) or \`line-soft\` (row separators).

**Usage.** \`<Separator />\` between sections; \`<Separator orientation="vertical" />\` between items in a row.

**Do** separate groups that differ, and prefer space alone where it is enough.

**Don't** draw a separator above every item, or use one as decoration.`,
      },
    },
  },
  argTypes: {
    tone: { control: "inline-radio", options: ["default", "soft"] },
    orientation: { control: "inline-radio", options: ["horizontal", "vertical"] },
  },
} satisfies Meta<typeof Separator>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Horizontal: Story = {
  render: (args) => (
    <div className="flex max-w-md flex-col gap-3">
      <Text>Clearance delivery</Text>
      <Separator {...args} />
      <Text>Ground control</Text>
    </div>
  ),
};

export const Soft: Story = { ...Horizontal, args: { tone: "soft" } };

export const Vertical: Story = {
  render: () => (
    <div className="flex h-6 items-center gap-3">
      <Text size="sm">12 items</Text>
      <Separator orientation="vertical" />
      <Text size="sm">3 complete</Text>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col gap-3">
        <Text size="sm">Line</Text>
        <Separator />
        <Text size="sm">Line soft</Text>
        <Separator tone="soft" />
      </div>
    </ThemeMatrix>
  ),
};
