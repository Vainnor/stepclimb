// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, waitFor } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Skeleton } from "./skeleton";

/** Waits out the 300 ms before a skeleton appears, so the screenshot has it. */
async function shown({ canvasElement }: { canvasElement: HTMLElement }) {
  await waitFor(() => expect(canvasElement.querySelector('[data-slot="skeleton"]')).not.toBeNull());
}

const meta = {
  title: "Data/Skeleton",
  component: Skeleton,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A still placeholder in the shape of content that is loading. It appears only once the load has taken 300 ms, so a fast load never flashes it, and it never shimmers. Screen readers skip it.

**Usage.** \`{query.isPending ? <Skeleton className="h-5 w-48" /> : title}\`, inside a region with \`aria-busy\` while it loads. \`shape\` is \`text\` (a line), \`block\` or \`circle\`; size it with \`className\`.

**Do** match the shape and size of what will replace it, so nothing moves when it loads.

**Don't** animate it, or use it for a wait whose length is unknown and long (say what is happening instead).`,
      },
    },
  },
  args: { className: "w-64" },
  argTypes: { shape: { control: "inline-radio", options: ["text", "block", "circle"] } },
  play: shown,
} satisfies Meta<typeof Skeleton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Text: Story = {};

/** A course row while it loads. */
export const Row: Story = {
  render: () => (
    <div aria-busy="true" className="flex max-w-md items-center gap-3">
      <Skeleton shape="circle" className="size-10" />
      <div className="flex flex-1 flex-col gap-2">
        <Skeleton className="w-3/4" />
        <Skeleton className="w-1/2" />
      </div>
    </div>
  ),
};

export const Block: Story = { args: { shape: "block", className: "h-32 max-w-md" } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex items-center gap-3">
        <Skeleton shape="circle" className="size-10" />
        <div className="flex flex-1 flex-col gap-2">
          <Skeleton className="w-3/4" />
          <Skeleton className="w-1/2" />
        </div>
      </div>
    </ThemeMatrix>
  ),
};
