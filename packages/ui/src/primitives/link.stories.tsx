// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Link } from "./link";
import { Text } from "./text";

const meta = {
  title: "Foundations/Link",
  component: Link,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch", "Docs"] },
    docs: {
      description: {
        component: `A link. An in-app \`href\` navigates through the app's React Aria \`RouterProvider\` (wired to TanStack Router), so it does not reload the page. A link to another site adds "(external site)" for screen readers, from the strings catalog.

**Usage.** \`<Link href="/courses/7">Ground control</Link>\`.

**Do** write link text that names where it goes, and use a link for navigation.

**Don't** write "click here", use a link for an action (that is a \`Button\`), or open another site in a new tab without saying so.`,
      },
    },
  },
  args: { href: "/courses/7", children: "Ground control" },
} satisfies Meta<typeof Link>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

/** A link inside running text. */
export const InText: Story = {
  render: (args) => (
    <Text elementType="p">
      Finish <Link {...args} /> before you start local control.
    </Text>
  ),
};

/** Another site: screen readers hear "(external site)" after the text. */
export const External: Story = {
  args: { href: "https://example.org/charts", children: "Airport charts" },
};

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("link")).toHaveAttribute("data-focus-visible");
  },
};

export const Disabled: Story = { args: { isDisabled: true } };

export const LongContent: Story = {
  render: (args) => (
    <div className="max-w-xs">
      <Link {...args}>
        Clearance delivery, ground control and local control: who talks to whom and when
      </Link>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Text elementType="p">
        Finish <Link href="/courses/7">Ground control</Link> first, or read the{" "}
        <Link href="https://example.org/charts">airport charts</Link>.
      </Text>
      <Link href="/courses/7" isDisabled>
        Local control
      </Link>
    </ThemeMatrix>
  ),
};
