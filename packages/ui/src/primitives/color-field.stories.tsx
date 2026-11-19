// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { type Color, parseColor } from "react-aria-components";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ColorField } from "./color-field";

/**
 * A sample value from its 24-bit RGB number: a brand color is data the field edits, and the UI
 * lint refuses a hex string anywhere outside packages/tokens.
 */
const rgb = (n: number): Color => parseColor(`#${n.toString(16).padStart(6, "0")}`);
const teal = rgb(0x0b6e75);

const meta = {
  title: "Forms/ColorField",
  component: ColorField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled color typed as six hex digits, with or without "#", and a swatch of it beside the input. The color is committed on blur or Enter; text that is not a color puts the last one back. The swatch is hidden from screen readers, which hear the input's value.

**Usage.** \`<ColorField label={t("branding.primary")} value={primary} onChange={(color) => setPrimary(color?.toString("hex") ?? null)} />\`. Values are React Aria's \`Color\`, or a hex string going in; \`onChange\` gives null when the input is cleared.

**Do** show what the color does next to the field (the branding screen's preview), and the server's refusal of a color as \`errorMessage\`.

**Don't** pass on a color without checking it where contrast matters: the field takes any color, and the theme generator says which work.`,
      },
    },
  },
  args: { label: "Brand color", defaultValue: teal, className: "max-w-60" },
} satisfies Meta<typeof ColorField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Empty: Story = {
  args: { defaultValue: undefined, description: "Six hex digits, such as the one in your logo." },
};

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("textbox")).toHaveFocus();
  },
};

export const Invalid: Story = {
  args: {
    defaultValue: rgb(0xf4e04d),
    isInvalid: true,
    errorMessage: "Text on this color is too hard to read. Choose a darker one.",
  },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const LongContent: Story = {
  args: {
    label: "The color of buttons, links and the progress rail across your organization",
    description: "Every page uses it. The preview shows how it looks in light and dark mode.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ColorField label="Brand color" defaultValue={teal} className="max-w-60" />
      <ColorField
        label="Brand color"
        defaultValue={rgb(0xf4e04d)}
        isInvalid
        errorMessage="Choose a darker color."
        className="max-w-60"
      />
    </ThemeMatrix>
  ),
};
