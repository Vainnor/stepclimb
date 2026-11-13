// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, CheckCircleIcon, ClockIcon, LockIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Icon } from "./icon";

const meta = {
  title: "Foundations/Icon",
  component: Icon,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A Phosphor icon at a token size. Phosphor is the only icon set; import the icon from \`@phosphor-icons/react\` and draw it through \`Icon\`.

**Usage.** \`<Icon icon={CheckCircleIcon} state="complete" />\`. The weight encodes state where an icon shows one: \`available\` draws the regular weight, \`complete\` the fill weight. Without a \`label\` the icon is decorative and hidden from screen readers; give it a \`label\` from \`useStepclimbStrings()\` when it carries meaning on its own.

**Do** pair an icon with text, use \`state\` for available and complete, and size it with \`size\` (or let it follow the text).

**Don't** use emoji or another icon set, color an icon to carry meaning alone, or leave an icon-only control without an accessible label.`,
      },
    },
  },
  args: { icon: BookOpenIcon },
  argTypes: {
    icon: { control: false },
    state: { control: "inline-radio", options: ["available", "complete"] },
    size: { control: "inline-radio", options: ["inherit", "sm", "md", "lg"] },
  },
} satisfies Meta<typeof Icon>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { size: "lg" } };

/** Regular for available, fill for complete. */
export const States: Story = {
  render: () => (
    <div className="flex items-center gap-4 text-fg-strong">
      <Icon icon={CheckCircleIcon} size="lg" />
      <Icon icon={CheckCircleIcon} size="lg" state="complete" />
    </div>
  ),
};

/** The token sizes, and `inherit`, which follows the text it sits in. */
export const Sizes: Story = {
  render: () => (
    <div className="flex items-end gap-4 text-fg-strong">
      <Icon icon={ClockIcon} size="sm" />
      <Icon icon={ClockIcon} size="md" />
      <Icon icon={ClockIcon} size="lg" />
      <span className="inline-flex items-center gap-1 text-2xl">
        <Icon icon={ClockIcon} />
        14:32
      </span>
    </div>
  ),
};

/** An icon that means something on its own is an image with a label. */
export const Labeled: Story = { args: { icon: LockIcon, size: "lg", label: "Locked" } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex items-center gap-4 text-fg-strong">
        <Icon icon={LockIcon} size="lg" />
        <Icon icon={CheckCircleIcon} size="lg" />
        <Icon icon={CheckCircleIcon} size="lg" state="complete" />
        <Icon icon={BookOpenIcon} size="md" />
      </div>
    </ThemeMatrix>
  ),
};
