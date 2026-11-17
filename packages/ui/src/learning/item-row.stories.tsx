// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ItemRow, type ItemRowItem } from "./item-row";

const now = new Date("2026-03-10T14:00:00Z");

const item: ItemRowItem = {
  id: "a",
  kind: "page",
  title: "Taxi instructions",
  href: "#taxi-instructions",
  state: "available",
  requirement: { kind: "mark_done" },
  due: new Date("2026-03-13T14:00:00Z"),
};

const meta = {
  title: "Learning/ItemRow",
  component: ItemRow,
  tags: ["experimental"],
  render: (args) => (
    <ol className="max-w-3xl">
      <ItemRow {...args} />
    </ol>
  ),
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `One item of a module on the course home: its kind's icon, its title, what finishes it, its learning state and, when it has one, its due date. The title is the link to the item view and is the link's whole name, so a page of rows never has two links that read the same; the kind, requirement, state and due label are its description. The whole row is the link's target. A locked item stays a link, since the item view says what unlocks it. A header item is a heading that groups the items after it.

Pressed draws as hover. Rows have no disabled, loading or error state: a locked item is still opened, and a page that is loading draws a \`Skeleton\` in place of the module.

**Usage.** Inside an \`ol\`, as \`ModuleRow\` lays them out: \`<ItemRow item={{ id, kind, title, href, state, requirement, due }} timeZone={me.tz} now={clock.now()} />\`. \`state\` is the learning state the server reports; \`due\` is the learner's due date, if the item has one.

**Do** pass the item view's path as \`href\` for every kind, external links included: the item view shows the link.

**Don't** hide locked items, or work out a state in the browser.`,
      },
    },
  },
  args: { item, timeZone: "America/New_York", now },
} satisfies Meta<typeof ItemRow>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

const stateItems: ItemRowItem[] = [
  {
    id: "1",
    kind: "page",
    title: "Radio basics",
    href: "#1",
    state: "complete",
    requirement: { kind: "view" },
  },
  {
    id: "2",
    kind: "video",
    title: "Phraseology",
    href: "#2",
    state: "progress",
    requirement: { kind: "watch", min_percent: 80 },
  },
  { id: "3", kind: "page", title: "Readback drill", href: "#3", state: "review" },
  {
    id: "4",
    kind: "file",
    title: "Airport diagram",
    href: "#4",
    state: "available",
    requirement: { kind: "mark_done" },
  },
  {
    id: "5",
    kind: "page",
    title: "Clearance delivery",
    href: "#5",
    state: "overdue",
    requirement: { kind: "mark_done" },
    due: new Date("2026-03-07T14:00:00Z"),
  },
  { id: "6", kind: "external_link", title: "Checkout booking", href: "#6", state: "locked" },
];

/** Each learning state, with the requirements the API accepts. */
export const AllStates: Story = {
  render: (args) => (
    <ol className="max-w-3xl">
      {stateItems.map((i) => (
        <ItemRow key={i.id} {...args} item={i} />
      ))}
    </ol>
  ),
};

const kindItems: ItemRowItem[] = [
  { id: "h", kind: "header", title: "Before the checkout" },
  { id: "p", kind: "page", title: "Page", href: "#p", state: "available" },
  { id: "f", kind: "file", title: "File", href: "#f", state: "available" },
  { id: "l", kind: "external_link", title: "External link", href: "#l", state: "available" },
  { id: "v", kind: "video", title: "Video", href: "#v", state: "available" },
  { id: "q", kind: "quiz", title: "Quiz", href: "#q", state: "available" },
  { id: "a", kind: "activity", title: "Activity", href: "#a", state: "available" },
];

/** Each kind's icon, and a header item above them. */
export const AllKinds: Story = {
  render: (args) => (
    <ol className="max-w-3xl">
      {kindItems.map((i) => (
        <ItemRow key={i.id} {...args} item={i} />
      ))}
    </ol>
  ),
};

const link = (canvasElement: HTMLElement) =>
  within(canvasElement).findByRole("link", { name: "Taxi instructions" });

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

/** The pointer over the row; a press draws the same. */
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

export const LongContent: Story = {
  args: {
    item: {
      ...item,
      title: "Simultaneous independent approaches to parallel runways in instrument conditions",
      requirement: { kind: "watch", min_percent: 100 },
      state: "progress",
      due: new Date("2026-04-30T14:00:00Z"),
      kind: "video",
    },
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <ol>
        <ItemRow {...args} />
        <ItemRow {...args} item={stateItems[4] as ItemRowItem} />
        <ItemRow {...args} item={stateItems[5] as ItemRowItem} />
      </ol>
    </ThemeMatrix>
  ),
};
