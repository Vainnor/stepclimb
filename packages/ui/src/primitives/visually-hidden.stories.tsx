// SPDX-License-Identifier: AGPL-3.0-or-later
import { XIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Icon } from "../theme/icon";
import { Button } from "./button";
import { VisuallyHidden } from "./visually-hidden";

const meta = {
  title: "Foundations/VisuallyHidden",
  component: VisuallyHidden,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch", "Docs"] },
    docs: {
      description: {
        component: `Content for screen readers only: it is in the accessibility tree and draws nothing. It has no matrix story, since it looks the same in every theme.

**Usage.** A label for a control whose visible content is an icon: \`<Button><Icon icon={XIcon} /><VisuallyHidden>{closeLabel}</VisuallyHidden></Button>\`, with \`closeLabel\` from \`useStepclimbStrings()\`. Use \`elementType="span"\` inside text.

**Do** take the hidden text from \`useStepclimbStrings()\`, and keep it short.

**Don't** hide information sighted people also need, or hide a control's only visible label.`,
      },
    },
  },
} satisfies Meta<typeof VisuallyHidden>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A button whose only visible content is an icon; screen readers hear "Close". */
export const IconButtonLabel: Story = {
  render: () => (
    <Button>
      <Icon icon={XIcon} size="sm" />
      <VisuallyHidden>Close</VisuallyHidden>
    </Button>
  ),
};
