// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { Button } from "../primitives/button";
import { DialogTrigger } from "../primitives/dialog";
import { Sheet } from "../primitives/sheet";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ProgressRail, type ProgressRailItem } from "./progress-rail";

const now = new Date("2026-03-10T14:00:00Z");

const items: ProgressRailItem[] = [
  { id: "a", title: "Radio basics", href: "#radio-basics", state: "complete" },
  { id: "b", title: "Phraseology", href: "#phraseology", state: "complete" },
  { id: "c", title: "Readback drill", href: "#readback-drill", state: "available" },
  {
    id: "d",
    title: "Clearance delivery",
    href: "#clearance-delivery",
    state: "overdue",
    due: new Date("2026-03-07T14:00:00Z"),
  },
  {
    id: "e",
    title: "Checkout",
    href: "#checkout",
    state: "locked",
    lockReasons: [{ kind: "sequential", itemTitle: "Clearance delivery" }],
  },
];

const meta = {
  title: "Learning/ProgressRail",
  component: ProgressRail,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A module's items drawn as a route: a line with one node per item, where the segment after a complete item fills with the brand color. Each node shows the item's learning state with its icon (complete, in progress, available, overdue with its due label, locked, needs review), and the current item is ringed. It is an ordered list of links: Up and Down move between them, Home and End jump to the ends. A locked item stays a focusable link and says what unlocks it, on hover and focus and to screen readers.

When an item completes while the rail is shown, its segment fills over 320 ms; when the course completes, every segment fills, one after another, within the same 320 ms. That is the only celebratory motion in Stepclimb, and it is instant with reduced motion.

**Usage.** \`<ProgressRail aria-label={module.title} items={railItems} currentId={item.id} courseComplete={course.complete} timeZone={me.tz} now={clock.now()} />\`. Resolve the lock reasons' module and item ids to the titles the learner sees. Beside the item view on wide screens; in a bottom \`Sheet\` behind a button under 768 px.

**Do** pass the state the server reports for each item, and the user's time zone.

**Don't** hide locked items, or animate anything else in their honor.`,
      },
    },
  },
  args: {
    "aria-label": "Ground control",
    items,
    currentId: "c",
    timeZone: "America/New_York",
    now,
  },
} satisfies Meta<typeof ProgressRail>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { className: "max-w-sm" } };

export const AllStates: Story = {
  args: {
    className: "max-w-sm",
    currentId: undefined,
    items: [
      { id: "1", title: "Complete", href: "#1", state: "complete" },
      { id: "2", title: "In progress", href: "#2", state: "progress" },
      { id: "3", title: "Needs review", href: "#3", state: "review" },
      { id: "4", title: "Available", href: "#4", state: "available" },
      {
        id: "5",
        title: "Overdue",
        href: "#5",
        state: "overdue",
        due: new Date("2026-03-09T14:00:00Z"),
      },
      {
        id: "6",
        title: "Locked",
        href: "#6",
        state: "locked",
        lockReasons: [{ kind: "module_locked" }],
      },
    ],
  },
};

const currentItems: ProgressRailItem[] = [
  { id: "available", title: "Available", href: "#1", state: "available" },
  { id: "progress", title: "In progress", href: "#2", state: "progress" },
  { id: "complete", title: "Complete", href: "#3", state: "complete" },
  { id: "review", title: "Needs review", href: "#4", state: "review" },
  {
    id: "overdue",
    title: "Overdue",
    href: "#5",
    state: "overdue",
    due: new Date("2026-03-09T14:00:00Z"),
  },
  {
    id: "locked",
    title: "Locked",
    href: "#6",
    state: "locked",
    lockReasons: [{ kind: "module_locked" }],
  },
];

/**
 * The current item in each state it can be in: ringed in the brand color, with a dot while
 * available. An overdue item keeps its overdue ring and gets the brand ring around it.
 */
export const Current: Story = {
  render: (args) => (
    <div className="flex flex-wrap gap-6">
      {currentItems.map((item) => (
        <ProgressRail
          {...args}
          key={item.id}
          aria-label={item.title}
          className="w-56"
          currentId={item.id}
          items={[item]}
        />
      ))}
    </div>
  ),
};

const readback = (canvasElement: HTMLElement) =>
  within(canvasElement).findByRole("link", { name: /^Readback drill/ });

/**
 * Hovers `link` until React Aria marks it hovered: a hover sent as the story first renders can
 * arrive before React Aria listens, and is then lost. It waits for the fonts and then for
 * the page to settle first: after a layout change, Chromium sends a pointer event for wherever
 * its own cursor rests, which ends React Aria's hover. A screen without hover (a phone) shows the
 * row as it is.
 */
async function hover(link: HTMLElement) {
  if (window.matchMedia("(hover: none)").matches) return;
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 300));
  await waitFor(async () => {
    await userEvent.unhover(link);
    await userEvent.hover(link);
    await expect(link).toHaveAttribute("data-hovered");
  });
}

/** The pointer over an unlocked item. */
export const Hover: Story = {
  args: { className: "max-w-sm", currentId: undefined },
  play: async ({ canvasElement }) => {
    await hover(await readback(canvasElement));
  },
};

/** A press held on an unlocked item. */
export const Pressed: Story = {
  args: { className: "max-w-sm", currentId: undefined },
  play: async ({ canvasElement }) => {
    const link = await readback(canvasElement);
    await userEvent.pointer({ keys: "[MouseLeft>]", target: link });
    await waitFor(() => expect(link).toHaveAttribute("data-pressed"));
    // A scripted press focuses the link as a keyboard would, and the browser then draws its
    // focus ring; a real mouse press draws none.
    link.blur();
    await expect(link).toHaveAttribute("data-pressed");
  },
};

/** Keyboard focus on an unlocked item: the ring sits inside the row. */
export const FocusVisible: Story = {
  args: { className: "max-w-sm", currentId: undefined },
  play: async ({ canvasElement }) => {
    const link = await readback(canvasElement);
    await userEvent.tab();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    await waitFor(() => expect(link).toHaveAttribute("data-focus-visible"));
  },
};

/** Keyboard focus and the pointer on the same item. */
export const HoverAndFocusVisible: Story = {
  args: { className: "max-w-sm", currentId: undefined },
  play: async ({ canvasElement }) => {
    const link = await readback(canvasElement);
    await userEvent.tab();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    await hover(link);
    await waitFor(() => expect(link).toHaveAttribute("data-focus-visible"));
  },
};

/** Keyboard focus on a locked item shows what unlocks it. */
export const LockedFocus: Story = {
  args: { className: "max-w-sm" },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    const first = await canvas.findByRole("link", { name: /^Radio basics/ });
    await userEvent.tab();
    await waitFor(() => expect(first).toHaveFocus());
    await userEvent.keyboard("{End}");
    const locked = canvas.getByRole("link", { name: /^Checkout/ });
    await waitFor(() => expect(locked).toHaveFocus());
    await within(document.body).findByRole("tooltip");
  },
};

export const LongContent: Story = {
  args: {
    className: "max-w-64",
    items: [
      {
        id: "a",
        title: "Simultaneous independent approaches to parallel runways in instrument conditions",
        href: "#a",
        state: "complete",
      },
      {
        id: "b",
        title: "Coordination with adjacent facilities during a ground stop",
        href: "#b",
        state: "overdue",
        due: new Date("2026-02-01T14:00:00Z"),
      },
      {
        id: "c",
        title: "Final checkout",
        href: "#c",
        state: "locked",
        lockReasons: [
          {
            kind: "prerequisites",
            moduleTitles: ["Ground control", "Clearance delivery", "Local control"],
          },
          { kind: "unlock_at", at: new Date("2026-04-01T13:00:00Z") },
        ],
      },
    ],
  },
};

export const Empty: Story = {
  args: { items: [], emptyState: "This module has no items yet." },
};

/** Press the button to complete the next item; completing the last completes the course. */
export const Completion: Story = {
  render: (args) => <CompletionDemo {...args} />,
};

function CompletionDemo(args: Parameters<typeof ProgressRail>[0]) {
  const [done, setDone] = useState(0);
  const steps: ProgressRailItem[] = [
    "Radio basics",
    "Phraseology",
    "Readback drill",
    "Checkout",
  ].map((title, i) => ({
    id: String(i),
    title,
    href: `#${i}`,
    state: i < done ? "complete" : "available",
  }));
  return (
    <div className="flex max-w-sm flex-col items-start gap-4">
      <ProgressRail
        {...args}
        items={steps}
        currentId={String(done)}
        courseComplete={done === steps.length}
      />
      <Button isDisabled={done === steps.length} onPress={() => setDone((d) => d + 1)}>
        Complete the next item
      </Button>
    </div>
  );
}

/** Under 768 px the item view opens the rail in a bottom sheet. */
export const InBottomSheet: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <DialogTrigger defaultOpen>
        <Button>Ground control</Button>
        <Sheet title="Ground control" placement="bottom">
          <ProgressRail {...args} />
        </Sheet>
      </DialogTrigger>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <ProgressRail {...args} />
    </ThemeMatrix>
  ),
};
