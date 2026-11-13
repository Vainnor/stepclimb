// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { Heading } from "./heading";
import { Popover, PopoverDialog, PopoverTrigger } from "./popover";
import { Text } from "./text";

const meta = {
  title: "Overlays/Popover",
  component: Popover,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A panel that floats beside the button that opened it, with the one soft shadow of a floating layer. Focus moves into it; Escape or a press outside closes it and returns focus to the button. \`Menu\` draws its list in one.

**Usage.** \`<PopoverTrigger><Button>Details</Button><Popover><PopoverDialog>…</PopoverDialog></Popover></PopoverTrigger>\`. Name the dialog with a \`Heading slot="title"\` inside it or with \`aria-label\`. \`placement\` sets the side; \`isOpen\` and \`onOpenChange\` on \`PopoverTrigger\` control it.

**Do** keep it short: a few lines of detail, or one or two controls.

**Don't** put work in it (editing and grading use pages or sheets), or nest a popover in a popover.`,
      },
    },
  },
} satisfies Meta<typeof Popover>;

export default meta;
type Story = StoryObj<typeof meta>;

function Example({ defaultOpen }: { defaultOpen?: boolean }) {
  return (
    <PopoverTrigger defaultOpen={defaultOpen}>
      <Button>Pass mark</Button>
      <Popover placement="bottom start" className="w-72">
        <PopoverDialog className="flex flex-col gap-2">
          <Heading slot="title" level={2} size="base">
            Pass mark
          </Heading>
          <Text size="sm" elementType="p">
            80 percent. Your best of three attempts counts toward the course.
          </Text>
          <Button slot="close" size="sm" className="self-start">
            Close
          </Button>
        </PopoverDialog>
      </Popover>
    </PopoverTrigger>
  );
}

export const Default: Story = { render: () => <Example /> };

export const Open: Story = {
  render: () => (
    <div className="min-h-48">
      <Example defaultOpen />
    </div>
  ),
};

/** The trigger in every brand, theme and density; the open panel is in the Open story's themes. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Example />
    </ThemeMatrix>
  ),
};
