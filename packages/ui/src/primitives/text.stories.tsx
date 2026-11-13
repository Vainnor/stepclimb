// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Text } from "./text";

const meta = {
  title: "Foundations/Text",
  component: Text,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Running text at a step of the type scale, in the UI face.

**Usage.** \`<Text size="sm" tone="sub">Due Friday</Text>\`. A \`span\` by default; \`elementType="p"\` for a paragraph. \`tone\` picks the text token: \`strong\` for emphasis, \`default\` for body, \`sub\` for secondary text, \`soft\` for placeholders and locked items.

**Do** change size and color through \`size\` and \`tone\`, and use \`Prose\` for long-form reading.

**Don't** pass color or font-size utilities in \`className\` (the lint warns), or use \`soft\` for text a person must read.`,
      },
    },
  },
  args: { children: "Complete the readback before you continue." },
  argTypes: {
    size: { control: "inline-radio", options: ["xs", "caption", "sm", "base", "lg"] },
    tone: { control: "inline-radio", options: ["strong", "default", "sub", "soft"] },
  },
} satisfies Meta<typeof Text>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Sizes: Story = {
  render: () => (
    <div className="flex flex-col gap-2">
      <Text size="lg">Large, 18 px</Text>
      <Text size="base">Base, 16 px</Text>
      <Text size="sm">Small, 14 px</Text>
      <Text size="caption">Caption, 13 px</Text>
      <Text size="xs">Extra small, 12 px</Text>
    </div>
  ),
};

export const Tones: Story = {
  render: () => (
    <div className="flex flex-col gap-2">
      <Text tone="strong">Strong: emphasis</Text>
      <Text tone="default">Default: body text</Text>
      <Text tone="sub">Sub: secondary text</Text>
      <Text tone="soft">Soft: placeholders and locked items</Text>
    </div>
  ),
};

export const LongContent: Story = {
  render: () => (
    <Text elementType="p" className="max-w-prose">
      Ground control issues taxi instructions to every aircraft moving on the movement area,
      including the runway to use, the route to it and any hold short instruction, and the pilot
      reads every hold short instruction back in full before moving. The text wraps at the width of
      its container.
    </Text>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col gap-1">
        <Text tone="strong">Strong text</Text>
        <Text>Default text</Text>
        <Text size="sm" tone="sub">
          Secondary text
        </Text>
        <Text size="caption" tone="soft">
          Soft caption
        </Text>
      </div>
    </ThemeMatrix>
  ),
};
