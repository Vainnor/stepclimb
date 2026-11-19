// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { AssignmentList, type AssignmentListEntry } from "./assignment-list";

const now = new Date("2026-03-10T14:00:00Z");

const assignments: AssignmentListEntry[] = [
  {
    id: "1",
    target: { type: "course", title: "S1 Ground theory" },
    href: "#s1",
    due: new Date("2026-03-13T14:00:00Z"),
    status: "in_progress",
  },
  {
    id: "2",
    target: { type: "module", title: "Clearance delivery" },
    courseTitle: "S2 Tower",
    href: "#clearance",
    due: new Date("2026-03-07T14:00:00Z"),
    status: "overdue",
  },
  {
    id: "3",
    target: { type: "item", title: "Taxi instructions" },
    courseTitle: "S1 Ground theory",
    href: "#taxi",
    due: null,
    status: "not_started",
  },
  {
    id: "4",
    target: { type: "course", title: "Annual refresher" },
    href: "#refresher",
    due: new Date("2026-03-01T14:00:00Z"),
    status: "completed",
  },
];

const meta = {
  title: "Learning/AssignmentList",
  component: AssignmentList,
  tags: ["experimental"],
  render: (args) => (
    <div className="max-w-3xl">
      <AssignmentList {...args} />
    </div>
  ),
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The caller's assignments: what someone must finish, and by when. Each row links to what was assigned (a course, a module or an item), names its course when the target is inside one, and shows its learning state and, until it is complete, its due date. Overdue rows come first, then rows by due date, then rows without one: the server's order, which the list keeps whatever order it is given. The server decides each row's status.

The title is the link's whole name; what was assigned, its course, its state and the due label are its description. The whole row is the link's target. Pressed draws as hover. Rows have no disabled or error state; a page that is loading draws a \`Skeleton\` in its place.

**Usage.** Map \`GET /api/v1/me/assignments\`'s rows to \`{ id, target: { type, title }, courseTitle, href, due, status }\` and render \`<AssignmentList assignments={rows} timeZone={me.tz} now={clock.now()} />\` under the page's heading.

**Do** pass the status the server sent.

**Don't** work out a status in the browser, or hide completed rows the server lists.`,
      },
    },
  },
  args: { assignments, timeZone: "America/New_York", now },
} satisfies Meta<typeof AssignmentList>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Every status, given in an order the list puts right: overdue first. */
export const Default: Story = {};

export const Empty: Story = { args: { assignments: [] } };

export const LongContent: Story = {
  args: {
    assignments: [
      {
        id: "1",
        target: {
          type: "item",
          title: "Simultaneous independent approaches to parallel runways in instrument conditions",
        },
        courseTitle: "S3 Approach control: radar vectoring, sequencing and the arrival flow",
        href: "#long",
        due: new Date("2026-04-30T14:00:00Z"),
        status: "in_progress",
      },
    ],
  },
};

const link = (canvasElement: HTMLElement) =>
  within(canvasElement).findByRole("link", { name: "Clearance delivery" });

/**
 * Hovers `target` until React Aria marks it hovered: a hover sent as the story first renders can
 * arrive before React Aria listens, and is then lost. It waits for the fonts and then for the page
 * to settle first: after a layout change, Chromium sends a pointer event for wherever its own
 * cursor rests, which ends React Aria's hover. A screen without hover (a phone) shows the row as
 * it is.
 */
async function hover(target: HTMLElement) {
  if (window.matchMedia("(hover: none)").matches) return;
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 300));
  await waitFor(async () => {
    await userEvent.unhover(target);
    await userEvent.hover(target);
    await expect(target).toHaveAttribute("data-hovered");
  });
}

/** The pointer over the first row; a press draws the same. */
export const Hover: Story = {
  play: async ({ canvasElement }) => {
    await hover(await link(canvasElement));
  },
};

/** Keyboard focus: the ring goes around the whole row. */
export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    const target = await link(canvasElement);
    await userEvent.tab();
    await waitFor(() => expect(target).toHaveAttribute("data-focus-visible"));
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <AssignmentList {...args} assignments={assignments.slice(0, 2)} />
    </ThemeMatrix>
  ),
};
