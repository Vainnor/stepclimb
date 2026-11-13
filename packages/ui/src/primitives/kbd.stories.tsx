// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Kbd } from "./kbd";
import { Text } from "./text";

const meta = {
  title: "Foundations/Kbd",
  component: Kbd,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A keyboard key or shortcut, in the mono face. It always reads left to right.

**Usage.** \`<Kbd>Ctrl K</Kbd>\`, one \`Kbd\` per key or one for the whole shortcut.

**Do** show shortcuts beside the action they trigger, and name keys as they are printed.

**Don't** use \`Kbd\` for code, IDs or other mono text (that is \`Text\` or \`Prose\`'s code), or make a shortcut the only way to an action.`,
      },
    },
  },
  args: { children: "Ctrl K" },
} satisfies Meta<typeof Kbd>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const InText: Story = {
  render: () => (
    <Text elementType="p">
      Press <Kbd>J</Kbd> and <Kbd>K</Kbd> to move through the queue, and <Kbd>Enter</Kbd> to save.
    </Text>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Text elementType="p">
        Search with <Kbd>Ctrl K</Kbd>, then <Kbd>Enter</Kbd>.
      </Text>
    </ThemeMatrix>
  ),
};
