// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Breadcrumb, Breadcrumbs } from "./breadcrumbs";

const meta = {
  title: "Navigation/Breadcrumbs",
  component: Breadcrumbs,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The path from the top of a section to the current page, in a navigation landmark named "Breadcrumbs". Each crumb before the last is a link; the last is the current page, marked as such for screen readers. On narrow screens the crumbs wrap.

**Usage.** \`<Breadcrumbs><Breadcrumb href="/courses">Courses</Breadcrumb><Breadcrumb href="/courses/s1">S1 Ground Theory</Breadcrumb><Breadcrumb>Taxi instructions</Breadcrumb></Breadcrumbs>\`.

**Do** use it on pages three or more levels deep, with each crumb named as its page's title.

**Don't** use it as the main navigation or to show steps of a process.`,
      },
    },
  },
  args: { children: null },
} satisfies Meta<typeof Breadcrumbs>;

export default meta;
type Story = StoryObj<typeof meta>;

const crumbs = (
  <Breadcrumbs>
    <Breadcrumb href="/courses">Courses</Breadcrumb>
    <Breadcrumb href="/courses/s1">S1 Ground Theory</Breadcrumb>
    <Breadcrumb>Taxi instructions</Breadcrumb>
  </Breadcrumbs>
);

export const Default: Story = { render: () => crumbs };

export const LongContent: Story = {
  render: () => (
    <Breadcrumbs>
      <Breadcrumb href="/courses">Courses</Breadcrumb>
      <Breadcrumb href="/courses/s1">S1 Ground Theory for clearance delivery and ground</Breadcrumb>
      <Breadcrumb href="/courses/s1/m2">
        Module 2: Ground control at airports with parallel runways
      </Breadcrumb>
      <Breadcrumb>Hold-short readbacks and runway crossing clearances</Breadcrumb>
    </Breadcrumbs>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => <ThemeMatrix>{crumbs}</ThemeMatrix>,
};
