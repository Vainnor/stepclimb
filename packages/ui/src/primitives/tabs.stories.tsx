// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Tab, TabList, TabPanel, Tabs } from "./tabs";
import { Text } from "./text";

const meta = {
  title: "Navigation/Tabs",
  component: Tabs,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Views of one subject that share a page, switched in place. The arrow keys move between tabs, Home and End jump to the first and last, and Tab moves into the panel. The selected tab has a brand underline and a darker label. A row wider than the screen scrolls sideways on its own.

**Usage.** \`<Tabs><TabList aria-label="Course"><Tab id="modules">Modules</Tab>…</TabList><TabPanel id="modules">…</TabPanel>…</Tabs>\`. \`selectedKey\` and \`onSelectionChange\` control it; \`defaultSelectedKey\` leaves it uncontrolled.

**Do** keep tab names short and the set small, and name the tab list when no heading says what it switches.

**Don't** use tabs to move between pages of the app (those are links) or for steps that must be done in order.`,
      },
    },
  },
} satisfies Meta<typeof Tabs>;

export default meta;
type Story = StoryObj<typeof meta>;

function Course({ long, ...props }: { long?: boolean; isDisabled?: boolean }) {
  return (
    <Tabs {...props}>
      <TabList aria-label="Course">
        <Tab id="modules">Modules</Tab>
        <Tab id="people">People</Tab>
        <Tab id="grades">Grades</Tab>
        {long ? <Tab id="settings">Completion requirements and certificates</Tab> : null}
        <Tab id="archive" isDisabled>
          Archive
        </Tab>
      </TabList>
      <TabPanel id="modules">
        <Text>Clearance delivery, Ground control and Local control.</Text>
      </TabPanel>
      <TabPanel id="people">
        <Text>Twelve learners and two instructors.</Text>
      </TabPanel>
      <TabPanel id="grades">
        <Text>Grades for every graded item.</Text>
      </TabPanel>
      <TabPanel id="settings">
        <Text>What a learner must finish to complete the course.</Text>
      </TabPanel>
      <TabPanel id="archive">
        <Text>Archived items.</Text>
      </TabPanel>
    </Tabs>
  );
}

export const Default: Story = { render: () => <Course /> };

export const FocusVisible: Story = {
  render: () => <Course />,
  // A Tab sent before the tabs are in the page focuses nothing, so the play waits for them first.
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const first = await canvas.findByRole("tab", { name: "Modules" });
    await userEvent.tab();
    await waitFor(() => expect(first).toHaveFocus());
    await userEvent.keyboard("{ArrowRight}");
    await waitFor(() => expect(canvas.getByRole("tab", { name: "People" })).toHaveFocus());
  },
};

/** Wider than a phone: the row scrolls sideways. */
export const LongContent: Story = { render: () => <Course long /> };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Course />
    </ThemeMatrix>
  ),
};
