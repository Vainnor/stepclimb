// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, HouseIcon, UserCircleIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Avatar } from "../primitives/avatar";
import { Button } from "../primitives/button";
import { Link } from "../primitives/link";
import { Text } from "../primitives/text";
import { AppShell } from "./app-shell";
import { BottomTabs } from "./bottom-tabs";
import { type NavEntry, SideNav } from "./side-nav";
import { TopBar } from "./top-bar";

const meta = {
  title: "Navigation/AppShell",
  component: AppShell,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The signed-in layout. Its first focusable element is a "Skip to main content" link, shown when focused. From 768 px the \`SideNav\` sits at the inline start (icons only until 1024 px); under 768 px \`BottomTabs\` replace it, and the content leaves room for them. The layout switches in CSS, so the first paint is already right.

**Usage.** \`<AppShell sideNav={<SideNav … />} bottomTabs={<BottomTabs … />} topBar={<TopBar … />}>{view}</AppShell>\`.

**Do** pass the same entries to the \`SideNav\` and the \`BottomTabs\`.

**Don't** wrap sign-in screens in it; they have their own layout.`,
      },
    },
  },
  args: { children: null },
} satisfies Meta<typeof AppShell>;

export default meta;
type Story = StoryObj<typeof meta>;

const entries: NavEntry[] = [
  { id: "home", href: "/", label: "Home", icon: HouseIcon },
  { id: "courses", href: "/courses", label: "Courses", icon: BookOpenIcon },
  { id: "account", href: "/me", label: "Account", icon: UserCircleIcon },
];

const moduleNumbers = Array.from({ length: 48 }, (_, i) => i + 1);

/**
 * A page longer than the screen with a button below the first screenful. Under 768 px, tabbing to
 * the button scrolls it into view above the BottomTabs, not under them.
 */
export const LongPage: Story = {
  render: () => (
    <AppShell
      className="min-h-[calc(100dvh-3rem)]"
      sideNav={
        <SideNav
          entries={entries}
          current="home"
          logo={<span className="text-lg font-heading text-fg-strong">Example Training</span>}
        />
      }
      bottomTabs={<BottomTabs entries={entries} current="home" />}
      topBar={<TopBar title="Home" />}
    >
      <div className="flex flex-col gap-3 p-6">
        {moduleNumbers.slice(0, 24).map((n) => (
          <Text key={n}>{`Module ${n}: an outline line that makes the page long.`}</Text>
        ))}
        <Button className="self-start">Save</Button>
        {moduleNumbers.slice(24).map((n) => (
          <Text key={n}>{`Module ${n}: an outline line below the button.`}</Text>
        ))}
      </div>
    </AppShell>
  ),
};

/** The SideNav at 1280 px, the BottomTabs at 375 px. */
export const Default: Story = {
  render: () => (
    // The screen's height less the story padding, so the story fits the viewport it is drawn in.
    <AppShell
      className="min-h-[calc(100dvh-3rem)]"
      sideNav={
        <SideNav
          entries={entries}
          current="home"
          logo={<span className="text-lg font-heading text-fg-strong">Example Training</span>}
          footer={<Link href="https://example.org/">Back to Example</Link>}
        />
      }
      bottomTabs={<BottomTabs entries={entries} current="home" />}
      topBar={<TopBar title="Home" actions={<Avatar name="Ada Lovelace" />} />}
    >
      <div className="flex flex-col gap-3 p-6">
        <Text>Next up: Module 2, Taxi instructions.</Text>
        <Text tone="sub">Due Thursday.</Text>
      </div>
    </AppShell>
  ),
};
