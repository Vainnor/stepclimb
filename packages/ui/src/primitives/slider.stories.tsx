// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Slider } from "./slider";

const meta = {
  title: "Forms/Slider",
  component: Slider,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `One number in a range, set by dragging the thumb or by typing the exact value into the number field beside it. Both show the same value. The arrow keys step the thumb, Page Up and Page Down by a tenth of the range, Home and End to the ends; a typed value is kept in range and on a step when it is committed.

**Usage.** \`<Slider label={t("quiz.passMark")} value={passMark} onChange={setPassMark} onChangeEnd={save} minValue={0} maxValue={1} step={0.05} formatOptions={{ style: "percent" }} />\`. \`onChange\` runs while the thumb moves; \`onChangeEnd\` once it is let go or a value is typed, so save there.

**Do** set \`minValue\`, \`maxValue\` and a \`step\` people can reach by dragging, and use \`formatOptions\` for percentages and units.

**Don't** use it where only the exact number matters and the range is wide or open (that is a \`NumberField\`), or for a range with two ends.`,
      },
    },
  },
  args: { label: "Pass mark", defaultValue: 80, minValue: 0, maxValue: 100, className: "max-w-96" },
} satisfies Meta<typeof Slider>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Percent: Story = {
  args: {
    defaultValue: 0.75,
    minValue: 0,
    maxValue: 1,
    step: 0.05,
    formatOptions: { style: "percent" },
  },
};

export const Minimum: Story = { args: { defaultValue: 0 } };

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("slider")).toHaveFocus();
  },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const LongContent: Story = {
  args: {
    label: "The distance from the runway threshold at which an approach must be stable",
    defaultValue: 1250,
    maxValue: 5000,
    step: 50,
    formatOptions: { style: "unit", unit: "meter" },
  },
};

/** Named by a heading elsewhere on the page, with no label of its own. */
export const LabelledBy: Story = {
  args: {
    label: undefined,
    "aria-labelledby": "slider-story-heading",
    defaultValue: 6,
    maxValue: 10,
  },
  render: (args) => (
    <section className="flex max-w-96 flex-col gap-2">
      <h3 id="slider-story-heading" className="text-sm font-medium text-fg-strong">
        Volume
      </h3>
      <Slider {...args} />
    </section>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Slider label="Pass mark" defaultValue={80} className="max-w-96" />
      <Slider label="Pass mark" defaultValue={80} isDisabled className="max-w-96" />
    </ThemeMatrix>
  ),
};
