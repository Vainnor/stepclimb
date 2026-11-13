// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { NumberField } from "./number-field";

const meta = {
  title: "Forms/NumberField",
  component: NumberField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled number input. React Aria parses and formats the number in the person's locale ("1.234,5" in German), and the digits are tabular so they line up. The arrow keys and the minus and plus buttons step it; the buttons are not tab stops.

**Usage.** \`<NumberField label={t("quiz.passMark")} defaultValue={0.8} formatOptions={{ style: "percent" }} minValue={0} maxValue={1} step={0.05} />\`. \`value\` and \`onChange\` control it.

**Do** set \`minValue\`, \`maxValue\` and \`step\`, and use \`formatOptions\` for percentages and units.

**Don't** use it for numbers that are identifiers, such as a CID or a phone number (that is a \`TextField\`).`,
      },
    },
  },
  args: {
    label: "Attempts allowed",
    defaultValue: 3,
    minValue: 1,
    maxValue: 10,
    className: "max-w-48",
  },
} satisfies Meta<typeof NumberField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Percent: Story = {
  args: {
    label: "Pass mark",
    defaultValue: 0.8,
    minValue: 0,
    maxValue: 1,
    step: 0.05,
    formatOptions: { style: "percent" },
  },
};

export const Empty: Story = { args: { defaultValue: undefined, placeholder: "1 to 10" } };

export const Invalid: Story = {
  args: { isInvalid: true, errorMessage: "Allow between 1 and 10 attempts." },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <NumberField label="Attempts allowed" defaultValue={3} className="max-w-48" />
      <NumberField
        label="Pass mark"
        defaultValue={0.8}
        formatOptions={{ style: "percent" }}
        isInvalid
        errorMessage="Set a pass mark from 50 to 100 percent."
        className="max-w-48"
      />
    </ThemeMatrix>
  ),
};
