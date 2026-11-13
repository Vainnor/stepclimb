// SPDX-License-Identifier: AGPL-3.0-or-later
import { BellIcon, BookOpenIcon, HouseIcon, UserCircleIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { BottomTabs } from "./bottom-tabs";
import type { NavEntry } from "./side-nav";

const meta = {
  title: "Navigation/BottomTabs",
  component: BottomTabs,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The app's main navigation on phones, under 768 px. They are links, not ARIA tabs: each goes to a page. The bar is fixed to the bottom above the home indicator and does not bounce. The current entry is marked for screen readers and has a brand bar above its icon and a heavier label.

**Usage.** \`<BottomTabs entries={entries} current="home" />\`, with the same entries as the \`SideNav\`. \`AppShell\` shows it only under 768 px.

**Do** keep it to five entries or fewer, with one-word labels.

**Don't** put actions in it; it only navigates.`,
      },
    },
  },
  args: { entries: [] },
} satisfies Meta<typeof BottomTabs>;

export default meta;
type Story = StoryObj<typeof meta>;

const entries: NavEntry[] = [
  { id: "home", href: "/", label: "Home", icon: HouseIcon },
  { id: "courses", href: "/courses", label: "Courses", icon: BookOpenIcon },
  { id: "notifications", href: "/notifications", label: "Notifications", icon: BellIcon },
  { id: "account", href: "/me", label: "Account", icon: UserCircleIcon },
];

/** Fixed to the bottom of the screen. The story fills the viewport so the screenshot holds it. */
export const Default: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <BottomTabs entries={entries} current="home" />
    </div>
  ),
};

/** In the matrix the bar is drawn in place rather than fixed. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <BottomTabs entries={entries} current="courses" className="static" />
    </ThemeMatrix>
  ),
};
