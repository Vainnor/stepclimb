// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Avatar } from "../primitives/avatar";
import { Button } from "../primitives/button";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { TopBar } from "./top-bar";

const meta = {
  title: "Navigation/TopBar",
  component: TopBar,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The header above the current view: its title, then the view's actions and the user menu at the inline end. On phones it sticks to the top under the status bar, over a translucent, blurred canvas.

**Usage.** \`<TopBar title="S1 Ground Theory" actions={<UserMenu />} />\`. The title is the page's \`h1\`; \`titleLevel\` changes that when the view has its own.

**Do** use the view's own name as the title.

**Don't** put navigation in it; that is the \`SideNav\` and \`BottomTabs\`.`,
      },
    },
  },
  args: { title: "S1 Ground Theory" },
} satisfies Meta<typeof TopBar>;

export default meta;
type Story = StoryObj<typeof meta>;

const actions = (
  <>
    <Button size="sm">Share</Button>
    <Avatar name="Ada Lovelace" />
  </>
);

export const Default: Story = { args: { actions } };

export const LongContent: Story = {
  args: {
    title: "S1 Ground Theory: clearance delivery, ground control and the airport movement area",
    actions,
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <TopBar title="S1 Ground Theory" titleLevel={3} actions={actions} />
    </ThemeMatrix>
  ),
};
