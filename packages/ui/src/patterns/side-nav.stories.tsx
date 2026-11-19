// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, HouseIcon, UserCircleIcon, UsersThreeIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { Link } from "../primitives/link";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { type NavEntry, SideNav } from "./side-nav";

const meta = {
  title: "Navigation/SideNav",
  component: SideNav,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The app's main navigation at the inline start of the screen from 768 px. The organization's logo sits at the top and its back link, when it has one, at the bottom. The current entry is marked for screen readers and drawn in the brand tint with a heavier label. From 768 to 1023 px it shows icons only; each label stays the link's name and shows as a tooltip. Under 768 px \`AppShell\` swaps it for \`BottomTabs\`.

**Usage.** \`<SideNav entries={entries} current="courses" logo={<img … />} footer={<Link href="https://example.org">Back to Example</Link>} />\`, where each entry is \`{ id, href, label, icon }\`. \`collapsed\` fixes the icon-only form; without it the width decides.

**Do** build the entries from routes the person can reach, with labels from the strings catalog.

**Don't** list a page that does not exist yet, or nest entries.`,
      },
    },
  },
  args: { entries: [] },
} satisfies Meta<typeof SideNav>;

export default meta;
type Story = StoryObj<typeof meta>;

const entries: NavEntry[] = [
  { id: "home", href: "/", label: "Home", icon: HouseIcon },
  { id: "courses", href: "/courses", label: "Courses", icon: BookOpenIcon },
  { id: "people", href: "/people", label: "People", icon: UsersThreeIcon },
  { id: "account", href: "/me", label: "Account", icon: UserCircleIcon },
];

const logo = <span className="text-lg font-heading text-fg-strong">Example Training</span>;
const back = <Link href="https://example.org/">Back to Example</Link>;

export const Default: Story = {
  render: () => (
    <SideNav entries={entries} current="courses" logo={logo} footer={back} collapsed={false} />
  ),
};

/** Icons only, as from 768 to 1023 px. */
export const Collapsed: Story = {
  render: () => <SideNav entries={entries} current="courses" collapsed />,
};

/** A collapsed entry shows its label as a tooltip on keyboard focus. */
export const CollapsedFocus: Story = {
  render: () => <SideNav entries={entries} current="courses" collapsed />,
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("link", { name: "Home" })).toHaveFocus();
    const tooltip = await within(document.body).findByRole("tooltip");
    await waitFor(() => expect(tooltip).toBeVisible());
  },
};

export const LongContent: Story = {
  render: () => (
    <SideNav
      entries={[
        ...entries,
        {
          id: "long",
          href: "/compliance",
          label: "Training compliance across every facility",
          icon: BookOpenIcon,
        },
      ]}
      current="long"
      logo={logo}
      footer={back}
      collapsed={false}
    />
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex h-96 gap-4">
        <SideNav
          entries={entries}
          current="courses"
          logo={logo}
          collapsed={false}
          className="h-full"
        />
        <SideNav entries={entries} current="courses" collapsed className="h-full" />
      </div>
    </ThemeMatrix>
  ),
};
