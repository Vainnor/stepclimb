// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Heading } from "./heading";

const meta = {
  title: "Foundations/Heading",
  component: Heading,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A heading. Its \`level\` (\`h1\` to \`h6\`) is required and separate from its \`size\`, so the page outline stays right whatever size a design asks for.

**Usage.** \`<Heading level={2}>Ground control</Heading>\`. Without \`size\`, each level has a step of the type scale (\`headingSizes\`); \`size\` overrides it without changing the level.

**Do** pick the level from the page outline (one \`h1\`, no skipped levels) and the size from the design.

**Don't** pick a level for its size, or bold a \`Text\` to stand in for a heading.`,
      },
    },
  },
  args: { level: 2, children: "Ground control" },
  argTypes: {
    level: { control: "inline-radio", options: [1, 2, 3, 4, 5, 6] },
    size: {
      control: "inline-radio",
      options: [undefined, "base", "lg", "xl", "2xl", "3xl", "4xl"],
    },
  },
} satisfies Meta<typeof Heading>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

/** Each level at its default size. */
export const Levels: Story = {
  render: () => (
    <div className="flex flex-col gap-3">
      <Heading level={1}>Level 1: S1 Ground theory</Heading>
      <Heading level={2}>Level 2: Ground control</Heading>
      <Heading level={3}>Level 3: Taxi instructions</Heading>
      <Heading level={4}>Level 4: Hold short</Heading>
      <Heading level={5}>Level 5: Readback</Heading>
      <Heading level={6}>Level 6: Notes</Heading>
    </div>
  ),
};

/** A level 2 heading drawn at the large size: the outline keeps level 2. */
export const SizeSeparateFromLevel: Story = { args: { level: 2, size: "lg" } };

export const LongContent: Story = {
  args: {
    level: 1,
    children:
      "Clearance delivery, ground control and local control: the positions, their frequencies and who talks to whom",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col gap-2">
        <Heading level={2}>Ground control</Heading>
        <Heading level={3}>Taxi instructions</Heading>
      </div>
    </ThemeMatrix>
  ),
};
