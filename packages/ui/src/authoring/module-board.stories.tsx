// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { Button } from "../primitives/button";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { applyMove, type BoardModule, type BoardMove } from "./board-moves";
import { ModuleBoard, type ModuleBoardProps, type MoveOutcome } from "./module-board";

const course: BoardModule[] = [
  {
    id: "m1",
    title: "Radio basics",
    published: true,
    unlockAt: null,
    prerequisiteIds: [],
    requireSequential: false,
    items: [
      {
        id: "i1",
        kind: "page",
        title: "Phraseology",
        published: true,
        requirement: { kind: "view" },
      },
      {
        id: "i2",
        kind: "video",
        title: "Readbacks",
        published: true,
        requirement: { kind: "watch", min_percent: 80 },
      },
      { id: "i3", kind: "header", title: "Practice", published: true, requirement: null },
      {
        id: "i4",
        kind: "external_link",
        title: "Frequency list",
        published: false,
        requirement: null,
      },
    ],
  },
  {
    id: "m2",
    title: "Ground control",
    published: false,
    unlockAt: new Date("2026-03-03T15:00:00Z"),
    prerequisiteIds: ["m1"],
    requireSequential: true,
    items: [
      {
        id: "i5",
        kind: "file",
        title: "Taxi diagram",
        published: true,
        requirement: { kind: "mark_done" },
      },
    ],
  },
  {
    id: "m3",
    title: "Clearance delivery",
    published: false,
    unlockAt: null,
    prerequisiteIds: [],
    requireSequential: false,
    items: [],
  },
];

/** The three writes against a copy kept in the story, as the server would answer them. */
function useStoryServer(answer: (move: BoardMove) => Promise<MoveOutcome> | null = () => null) {
  const [modules, setModules] = useState(course);
  const write = (move: BoardMove) => {
    const given = answer(move);
    if (given !== null) return given;
    setModules((m) => applyMove(m, move));
    return Promise.resolve<MoveOutcome>({ status: "moved" });
  };
  return {
    modules,
    onReorderModules: (moduleIds: string[]) => write({ kind: "modules", moduleIds }),
    onReorderItems: (moduleId: string, itemIds: string[]) =>
      write({ kind: "items", moduleId, itemIds }),
    onMoveItem: (itemId: string, to: { moduleId: string; position: number }) =>
      write({ kind: "move", itemId, ...to }),
    onReload: () => setModules(course),
  };
}

type BoardArgs = Pick<
  ModuleBoardProps,
  "isDisabled" | "defaultEditingItemId" | "emptyState" | "timeZone" | "className"
>;

function Board({
  answer,
  modules,
  ...args
}: BoardArgs & {
  answer?: (move: BoardMove) => Promise<MoveOutcome> | null;
  modules?: BoardModule[];
}) {
  const server = useStoryServer(answer);
  return (
    <ModuleBoard
      {...server}
      {...args}
      modules={modules ?? server.modules}
      itemEditor={(item, { close }) => (
        <div className="flex flex-col items-start gap-4">
          <p>{item.title}</p>
          <Button onPress={close}>Done</Button>
        </div>
      )}
    />
  );
}

const meta = {
  title: "Authoring/ModuleBoard",
  component: Board,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The course structure editor: each module's row, then its items' rows, in one list. A row moves by dragging it, by its handle from the keyboard (Enter, the arrow keys to a place, Enter), or from its Move menu: Move up, Move down and, for an item, Move to module, which puts it at the end of that module. Each move is one write: the modules' order, a module's items' order, or an item's move to another module (spec 03 B10, B11). The board shows the new order while the server answers, says the result in its status line, and puts the order back on a refusal. Rows show the publish state, an item's requirement and a module's unlock rules; Edit opens the item's editor in a side sheet.

Hover, pressed and disabled of the handle and the buttons are those of Button and IconButton; no story captures them here, so States is listed as missing.

**Usage.** \`<ModuleBoard modules={modules} onReorderModules={(ids) => put(...)} onReorderItems={(moduleId, ids) => put(...)} onMoveItem={(itemId, { moduleId, position }) => post(...)} onReload={refetch} itemEditor={(item, { close }) => <ItemForm ... />} timeZone={me.tz} />\`. Each handler resolves \`{ status: "moved" }\` once \`modules\` holds the server's answer, \`{ status: "stale" }\` on 409 \`order.stale\`, or \`{ status: "failed", message }\` with the message to show for any other refusal.

**Do** pass \`isDisabled\` for an archived course, where every write is refused.

**Don't** apply a move to \`modules\` before the server accepts it: the board already shows it meanwhile.`,
      },
    },
  },
  args: { timeZone: "America/New_York", className: "max-w-3xl" },
} satisfies Meta<typeof Board>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A course with a header item, an unpublished item, unlock rules and an empty module. */
export const Default: Story = {};

const row = (canvas: HTMLElement, name: RegExp) => within(canvas).findByRole("row", { name });

/** Keyboard focus on a row. */
export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    const target = await row(canvasElement, /^1 Radio basics/);
    await waitFor(() => expect(target).toHaveAttribute("data-focus-visible"));
  },
};

/** An item's Move menu, with Move up unavailable at the top of its module. */
export const MoveMenu: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.click(
      await within(canvasElement).findByRole("button", { name: "Move Phraseology" }),
    );
    const body = within(canvasElement.ownerDocument.body);
    await body.findByRole("menuitem", { name: "Move to module" });
  },
};

const moveDown = async (canvasElement: HTMLElement) => {
  await userEvent.click(
    await within(canvasElement).findByRole("button", { name: "Move Phraseology" }),
  );
  const body = within(canvasElement.ownerDocument.body);
  await userEvent.click(await body.findByRole("menuitem", { name: "Move down" }));
};

/** A move the server has not answered yet: shown in its new place, other moves held back. */
export const Saving: Story = {
  render: (args) => <Board {...args} answer={() => new Promise<MoveOutcome>(() => {})} />,
  play: async ({ canvasElement }) => {
    await moveDown(canvasElement);
    await within(canvasElement).findByText("Saving the new order.");
  },
};

/** 409 `order.stale`: another author changed the order, so the board reloads and says so. */
export const Stale: Story = {
  render: (args) => (
    <Board {...args} answer={() => Promise.resolve<MoveOutcome>({ status: "stale" })} />
  ),
  play: async ({ canvasElement }) => {
    await moveDown(canvasElement);
    await within(canvasElement).findByRole("alert");
  },
};

/** An archived course: every write is refused, so nothing moves. */
export const Disabled: Story = { args: { isDisabled: true } };

/** An item's editor open in the side sheet. */
export const Editing: Story = {
  args: { defaultEditingItemId: "i5" },
  play: async ({ canvasElement }) => {
    await within(canvasElement.ownerDocument.body).findByRole("dialog");
  },
};

export const Empty: Story = {
  render: (args) => <Board {...args} modules={[]} emptyState="No modules yet." />,
};

const long: BoardModule[] = [
  {
    id: "l1",
    title: "Coordination with adjacent facilities during a ground stop and the arrival flow",
    published: false,
    unlockAt: new Date("2026-04-01T13:00:00Z"),
    prerequisiteIds: ["l0"],
    requireSequential: true,
    items: [
      {
        id: "l2",
        kind: "activity",
        title: "Simultaneous independent approaches to parallel runways in instrument conditions",
        published: false,
        requirement: { kind: "mark_done" },
      },
    ],
  },
  {
    id: "l0",
    title: "Approach control fundamentals for the terminal radar approach control facility",
    published: true,
    unlockAt: null,
    prerequisiteIds: [],
    requireSequential: false,
    items: [],
  },
];

export const LongContent: Story = {
  render: (args) => <Board {...args} modules={long} />,
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <Board {...args} className="w-full" />
    </ThemeMatrix>
  ),
};
