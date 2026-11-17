// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import type { ItemRowItem } from "./item-row";
import { ModuleRow } from "./module-row";

const now = new Date("2026-03-10T14:00:00Z");

const items: ItemRowItem[] = [
  {
    id: "a",
    kind: "page",
    title: "Taxi routes",
    href: "#a",
    state: "complete",
    requirement: { kind: "view" },
  },
  {
    id: "b",
    kind: "video",
    title: "Taxi instructions",
    href: "#b",
    state: "progress",
    requirement: { kind: "watch", min_percent: 80 },
  },
  { id: "h", kind: "header", title: "Practice" },
  {
    id: "c",
    kind: "page",
    title: "Hold short readbacks",
    href: "#c",
    state: "available",
    requirement: { kind: "mark_done" },
    due: new Date("2026-03-13T14:00:00Z"),
  },
  { id: "d", kind: "file", title: "Airport diagram", href: "#d", state: "available" },
];

const meta = {
  title: "Learning/ModuleRow",
  component: ModuleRow,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `One module on the course home: its number, title, learning state and the learner's progress ("3 / 5"), or, while it is locked, what it waits for in words ("Unlocks after Ground control", "Opens on Mar 3, 2026, 10:00 AM EST" in the user's time zone). Its items follow as \`ItemRow\`s, which hold the module's only controls; their hover and focus states are in Learning/ItemRow.

**Usage.** \`<ModuleRow number={module.position + 1} title={module.title} state={state} lockReasons={reasons} progress={{ completed, required }} items={rows} timeZone={me.tz} now={clock.now()} />\`. Resolve the lock reasons' module ids to the titles the learner sees. Leave \`progress\` out for staff, who have none.

**Do** keep a locked module's items listed: the learner sees what is ahead.

**Don't** collapse modules behind a toggle, or color a module without its chip.`,
      },
    },
  },
  args: {
    number: 2,
    title: "Ground control",
    state: "progress",
    progress: { completed: 1, required: 4 },
    items,
    timeZone: "America/New_York",
    now,
    className: "max-w-3xl",
  },
} satisfies Meta<typeof ModuleRow>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

const lockedItems: ItemRowItem[] = [
  { id: "x", kind: "page", title: "Departure sequencing", href: "#x", state: "locked" },
  { id: "y", kind: "quiz", title: "Local control check", href: "#y", state: "locked" },
];

/** Waiting for a module and a date, with its items still listed. */
export const Locked: Story = {
  args: {
    number: 3,
    title: "Local control",
    state: "locked",
    lockReasons: [
      { kind: "prerequisites", moduleTitles: ["Ground control"] },
      { kind: "unlock_at", at: new Date("2026-03-17T14:00:00Z") },
    ],
    items: lockedItems,
  },
};

const states = [
  ["complete", "Radio basics"],
  ["progress", "Ground control"],
  ["available", "Clearance delivery"],
  ["overdue", "Phraseology refresher"],
  ["review", "Readback check"],
] as const;

/** The header of a module in each state but locked, which the Locked story shows. */
export const AllStates: Story = {
  render: (args) => (
    <div className="flex max-w-3xl flex-col gap-4">
      {states.map(([state, title], i) => (
        <ModuleRow
          {...args}
          key={state}
          number={i + 1}
          title={title}
          state={state}
          lockReasons={undefined}
          progress={{ completed: state === "complete" ? 4 : i, required: 4 }}
          items={[]}
        />
      ))}
    </div>
  ),
};

export const Empty: Story = {
  args: {
    items: [],
    progress: undefined,
    state: "available",
    emptyState: "This module has no items yet.",
  },
};

export const LongContent: Story = {
  args: {
    number: 12,
    title: "Coordination with adjacent facilities during a ground stop and the arrival flow",
    state: "locked",
    lockReasons: [
      {
        kind: "prerequisites",
        moduleTitles: ["Ground control", "Clearance delivery", "Local control", "Approach control"],
      },
      { kind: "unlock_at", at: new Date("2026-04-01T13:00:00Z") },
    ],
    items: [
      {
        id: "l",
        kind: "activity",
        title: "Simultaneous independent approaches to parallel runways in instrument conditions",
        href: "#l",
        state: "locked",
        requirement: { kind: "mark_done" },
      },
    ],
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <ModuleRow {...args} className="w-full" />
    </ThemeMatrix>
  ),
};
