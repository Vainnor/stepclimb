// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Radio, RadioGroup } from "./radio-group";

const meta = {
  title: "Forms/RadioGroup",
  component: RadioGroup,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `One choice from a few options, all visible at once. Tab enters the group at the chosen option and the arrow keys move the choice. Each label is a press target at least 44 px tall on touch screens, and an option can carry its own help line.

**Usage.** \`<RadioGroup label="Feedback" value={v} onChange={setV}><Radio value="full">Full</Radio>…</RadioGroup>\`, or \`defaultValue\`. \`orientation="horizontal"\` lays short options in a row.

**Do** use it for two to five options people should compare before choosing.

**Don't** use it for one on-or-off choice (a \`Checkbox\` or \`Switch\`) or for a long list (a \`Select\`).`,
      },
    },
  },
  args: { label: "Feedback", children: null },
} satisfies Meta<typeof RadioGroup>;

export default meta;
type Story = StoryObj<typeof meta>;

const options = (
  <>
    <Radio value="full" description="A result after each answer, with the rule broken explained.">
      Full
    </Radio>
    <Radio value="result">Result only</Radio>
    <Radio value="none">None</Radio>
  </>
);

export const Default: Story = {
  render: (args) => (
    <RadioGroup {...args} description="What learners see after submitting." defaultValue="full">
      {options}
    </RadioGroup>
  ),
};

export const Horizontal: Story = {
  render: (args) => (
    <RadioGroup {...args} label="Density" orientation="horizontal" defaultValue="comfortable">
      <Radio value="comfortable">Comfortable</Radio>
      <Radio value="compact">Compact</Radio>
    </RadioGroup>
  ),
};

export const FocusVisible: Story = {
  render: (args) => (
    <RadioGroup {...args} defaultValue="result">
      {options}
    </RadioGroup>
  ),
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("radio", { name: "Result only" })).toHaveFocus();
  },
};

export const Invalid: Story = {
  render: (args) => (
    <RadioGroup {...args} isInvalid errorMessage="Choose what learners see after submitting.">
      {options}
    </RadioGroup>
  ),
};

export const Disabled: Story = {
  render: (args) => (
    <RadioGroup {...args} isDisabled defaultValue="none">
      {options}
    </RadioGroup>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <RadioGroup label="Feedback" defaultValue="full">
        {options}
      </RadioGroup>
      <RadioGroup label="Feedback" isInvalid errorMessage="Choose one.">
        <Radio value="a">Full</Radio>
        <Radio value="b" isDisabled>
          None
        </Radio>
      </RadioGroup>
    </ThemeMatrix>
  ),
};
