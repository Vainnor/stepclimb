// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { TagField } from "./tag-field";

const meta = {
  title: "Forms/TagField",
  component: TagField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled list of short words or phrases, typed one at a time and shown as removable tags under the input. Enter or the Add button adds what is typed; a blank tag, or one already in the list in any case, is not added. Each tag's remove button, or Backspace or Delete on a focused tag, removes it. The tags are one tab stop; the arrow keys move between them.

**Usage.** \`<TagField label={t("course.keywords")} value={keywords} onChange={setKeywords} />\`. Without \`value\`, \`defaultValue\` sets the first tags. \`isInvalid\` and \`errorMessage\` show the caller's own check, such as a limit on the number of tags.

**Do** keep tags short: a word or two that people filter or search by.

**Don't** use it to choose from a fixed list (that is a \`CheckboxGroup\`, or a \`ComboBox\` for a long one), or for sentences.`,
      },
    },
  },
  args: {
    label: "Keywords",
    defaultValue: ["Tower", "Ground", "Clearance delivery"],
    className: "max-w-96",
  },
} satisfies Meta<typeof TagField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  args: { defaultValue: [], description: "People can search the catalog by these words." },
};

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("row", { name: "Tower" })).toHaveFocus();
  },
};

/** A tag already in the list, typed again: it stays in the input with a note. */
export const Repeated: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    // From the keyboard: a pointer would leave the input hovered on some runs, and the hover
    // border replaces the focus border in the screenshot.
    await userEvent.tab();
    await userEvent.keyboard("tower{Enter}");
    await expect(canvas.getByRole("textbox")).toHaveValue("tower");
    // The note under the input; the live region repeats it out of sight.
    await expect(
      canvas.getByText("Already added", { selector: '[data-slot="tag-field-note"]' }),
    ).toBeVisible();
  },
};

export const Invalid: Story = {
  args: { isInvalid: true, errorMessage: "Use at most 2 keywords." },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const LongContent: Story = {
  args: {
    label: "Keywords people can search the training catalog by",
    defaultValue: [
      "Standard terminal arrival routes and the descents published with them",
      "Tower",
      "Ground",
    ],
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <TagField label="Keywords" defaultValue={["Tower", "Ground"]} className="max-w-96" />
      <TagField
        label="Keywords"
        defaultValue={["Tower", "Ground", "Approach"]}
        isInvalid
        errorMessage="Use at most 2 keywords."
        className="max-w-96"
      />
    </ThemeMatrix>
  ),
};
