// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Checkbox, CheckboxGroup } from "./checkbox";

const meta = {
  title: "Forms/Checkbox",
  component: Checkbox,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A choice that is on or off, saved with its form. Checked shows a check, mixed (some of what it stands for is checked) shows a dash, so color is never the only sign. The whole label is the press target, at least 44 px tall on touch screens. \`CheckboxGroup\` labels a set of checkboxes and holds their checked values.

**Usage.** \`<Checkbox isSelected={x} onChange={setX}>Published</Checkbox>\`, or \`defaultSelected\`. \`<CheckboxGroup label="Notify me about" value={v} onChange={setV}><Checkbox value="grades">Grades</Checkbox>…</CheckboxGroup>\`. \`isIndeterminate\` draws the mixed state, which you set and clear.

**Do** word the label so that checked means yes.

**Don't** use a checkbox for a setting that applies at once without saving (that is a \`Switch\`).`,
      },
    },
  },
  args: { children: "Published" },
} satisfies Meta<typeof Checkbox>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { description: "Learners see it in their course list." } };

export const Checked: Story = { args: { defaultSelected: true } };

export const Indeterminate: Story = { args: { children: "All modules", isIndeterminate: true } };

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("checkbox")).toHaveFocus();
  },
};

export const Invalid: Story = {
  args: {
    children: "I accept the honor code",
    isInvalid: true,
    errorMessage: "Accept the honor code to start the exam.",
  },
};

/** No visible label, as in a table's selection column: named by `aria-label`, and still a
 * 44 px target on touch screens. */
export const WithoutLabel: Story = {
  args: { children: undefined, "aria-label": "Select Ground control" },
};

export const Disabled: Story = { args: { isDisabled: true, defaultSelected: true } };

export const Group: Story = {
  render: () => (
    <CheckboxGroup
      label="Notify me about"
      description="By email and in the app."
      defaultValue={["grades"]}
    >
      <Checkbox value="grades">Grades</Checkbox>
      <Checkbox value="deadlines">Deadlines</Checkbox>
      <Checkbox value="announcements">Announcements</Checkbox>
    </CheckboxGroup>
  ),
};

export const LongContent: Story = {
  args: {
    children:
      "I have read the facility's standard operating procedures for ground control and agree to follow them during practice sessions",
    description: "Your instructor sees when you accepted.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col">
        <Checkbox>Published</Checkbox>
        <Checkbox defaultSelected>Graded</Checkbox>
        <Checkbox isIndeterminate>All modules</Checkbox>
        <Checkbox isInvalid errorMessage="Accept the honor code.">
          I accept the honor code
        </Checkbox>
        <Checkbox isDisabled defaultSelected>
          Locked
        </Checkbox>
      </div>
    </ThemeMatrix>
  ),
};
