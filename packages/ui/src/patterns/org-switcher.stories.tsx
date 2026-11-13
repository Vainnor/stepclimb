// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { OrgSwitcher } from "./org-switcher";

const meta = {
  title: "Navigation/OrgSwitcher",
  component: OrgSwitcher,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Moves between the organizations a person is signed in to on a multi-organization server. It shows the current organization; its menu lists the others, the current one checked. Choosing another calls \`onChange\`, and the app goes to that organization's address.

**Usage.** \`<OrgSwitcher organizations={orgs} current={org.id} onChange={(id) => go(id)} />\`.

**Do** render it only in multi-organization mode.

**Don't** render it, even hidden, when the server has one organization.`,
      },
    },
  },
  args: {
    organizations: [
      { id: "north", name: "North Training Group" },
      { id: "south", name: "South Flight School" },
    ],
    current: "north",
    onChange: () => {},
  },
} satisfies Meta<typeof OrgSwitcher>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Open: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <OrgSwitcher {...args} />
    </div>
  ),
  play: async ({ canvasElement }) => {
    await userEvent.click(within(canvasElement).getByRole("button"));
  },
};

export const LongContent: Story = {
  args: {
    organizations: [
      { id: "long", name: "Northern Regional Training Group for Air Traffic Services" },
      { id: "south", name: "South Flight School" },
    ],
    current: "long",
    className: "max-w-60",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <OrgSwitcher {...args} />
    </ThemeMatrix>
  ),
};
