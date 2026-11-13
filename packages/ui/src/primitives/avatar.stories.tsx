// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, waitFor } from "storybook/test";
import sample from "../storybook/avatar-sample.svg";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Avatar } from "./avatar";

const meta = {
  title: "Data/Avatar",
  component: Avatar,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A person's picture in a pill, or the initials of their display name when there is no picture or it fails to load. Screen readers hear the display name.

**Usage.** \`<Avatar name={user.displayName} src={user.avatarUrl} size="sm" />\`. Initials are the first letter of the first and the last word, so "Grace Brewster Hopper" shows "GH".

**Do** pass the name the person chose to display, and size it with \`size\`.

**Don't** use it for an organization or a course (they have logos), or color it to mean a state.`,
      },
    },
  },
  args: { name: "Ada Lovelace" },
  argTypes: { size: { control: "inline-radio", options: ["sm", "md", "lg"] } },
} satisfies Meta<typeof Avatar>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Initials: Story = {};

export const Picture: Story = {
  args: { src: sample },
  play: async ({ canvasElement }) => {
    const image = canvasElement.querySelector("img");
    await waitFor(() => expect(image?.complete && image.naturalWidth > 0).toBe(true));
  },
};

/** The picture failed to load: the initials take its place. */
export const BrokenPicture: Story = {
  args: { src: "/avatar-that-does-not-exist.png" },
  play: async ({ canvasElement }) => {
    await waitFor(() =>
      expect(canvasElement.querySelector('[data-slot="avatar-initials"]')).not.toBeNull(),
    );
  },
};

export const Sizes: Story = {
  render: (args) => (
    <div className="flex items-center gap-3">
      <Avatar {...args} size="sm" />
      <Avatar {...args} size="md" />
      <Avatar {...args} size="lg" />
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex items-center gap-3">
        <Avatar name="Ada Lovelace" size="sm" />
        <Avatar name="Grace Brewster Hopper" />
        <Avatar name="Émile Zola" size="lg" />
        <Avatar name="Ada Lovelace" src={sample} size="lg" />
      </div>
    </ThemeMatrix>
  ),
};
