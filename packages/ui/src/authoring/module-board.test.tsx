// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import type { BoardModule } from "./board-moves";
import { ModuleBoard, type ModuleBoardProps, type MoveOutcome } from "./module-board";

const NY = "America/New_York";

const radio: BoardModule = {
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
    { id: "i2", kind: "video", title: "Readbacks", published: false, requirement: null },
    { id: "i3", kind: "header", title: "Practice", published: true, requirement: null },
  ],
};

const ground: BoardModule = {
  id: "m2",
  title: "Ground control",
  published: false,
  unlockAt: new Date("2026-03-03T15:00:00Z"),
  prerequisiteIds: ["m1"],
  requireSequential: true,
  items: [
    {
      id: "i4",
      kind: "file",
      title: "Taxi diagram",
      published: true,
      requirement: { kind: "mark_done" },
    },
  ],
};

const modules: BoardModule[] = [radio, ground];

function setup(overrides: Partial<ModuleBoardProps> = {}) {
  const moved = async (): Promise<MoveOutcome> => ({ status: "moved" });
  const props: ModuleBoardProps = {
    modules,
    onReorderModules: vi.fn(moved),
    onReorderItems: vi.fn(moved),
    onMoveItem: vi.fn(moved),
    onReload: vi.fn(),
    timeZone: NY,
    ...overrides,
  };
  const view = renderWithStrings(<ModuleBoard {...props} />);
  return { props, view, user: userEvent.setup() };
}

const row = (name: RegExp) => screen.getByRole("row", { name });

async function choose(user: ReturnType<typeof userEvent.setup>, button: string, ...path: string[]) {
  await user.click(screen.getByRole("button", { name: button }));
  for (const item of path) await user.click(await screen.findByRole("menuitem", { name: item }));
}

describe("ModuleBoard", () => {
  // A wide screen: menus open in a popover and the sheet at the inline end.
  beforeEach(() => stubMatchMedia(() => true));
  afterEach(() => vi.unstubAllGlobals());

  it("lists modules and their items as rows of one grid, in course order", () => {
    setup();
    const grid = screen.getByRole("grid", { name: "Modules and items" });
    const names = within(grid)
      .getAllByRole("row")
      .map((r) => r.getAttribute("aria-label"));
    expect(names).toEqual([
      "1 Radio basics",
      "Phraseology",
      "Readbacks",
      "Practice",
      "2 Ground control",
      "Taxi diagram",
    ]);
    expect(screen.getByRole("heading", { level: 2, name: "2 Ground control" })).toBeInTheDocument();
  });

  it("says each row's publish state, requirement and unlock rules in one description", () => {
    setup();
    const description = (r: HTMLElement) => {
      const ids = (r.getAttribute("aria-labelledby") ?? "").split(" ");
      expect(ids).toHaveLength(2);
      return document.getElementById(ids[1] ?? "")?.textContent;
    };
    expect(description(row(/^Phraseology/))).toBe("Page, View, Published");
    expect(description(row(/^Readbacks/))).toBe("Video, Unpublished");
    expect(description(row(/^Practice/))).toBe("Header, Published");
    expect(description(row(/^2 Ground control/))).toBe(
      "Unpublished, Opens on Mar 3, 2026, 10:00 AM EST, Unlocks after Radio basics, Items in order",
    );
  });

  it("names every button after its row, so none repeats", () => {
    const twin = { ...ground, items: ground.items.map((i) => ({ ...i, title: "Phraseology" })) };
    setup({ modules: [radio, twin], itemEditor: () => null });
    const names = screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"));
    expect(new Set(names).size).toBe(names.length);
    expect(names).toContain("Move Phraseology, item 1 of module 1");
    expect(names).toContain("Edit Phraseology, item 1 of module 2");
    expect(names).toContain("Drag 2 Ground control");
  });

  it("moves an item down through the order route, once", async () => {
    const { props, user } = setup();
    await choose(user, "Move Phraseology", "Move down");
    expect(props.onReorderItems).toHaveBeenCalledTimes(1);
    expect(props.onReorderItems).toHaveBeenCalledWith("m1", ["i2", "i1", "i3"]);
    expect(props.onMoveItem).not.toHaveBeenCalled();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Moved Phraseology to position 2 of 3 in Radio basics.",
    );
  });

  describe("keyboard drag and drop", () => {
    /** Starts a keyboard drag from `row`'s handle; returns the drop targets offered, in order. */
    async function drag(user: ReturnType<typeof userEvent.setup>, name: string) {
      // Into the grid, down to the row, then right to its handle, as a keyboard user goes.
      await user.tab();
      while (label(document.activeElement) !== name) await user.keyboard("{ArrowDown}");
      await user.keyboard("{ArrowRight}");
      expect(document.activeElement).toBe(screen.getByRole("button", { name: `Drag ${name}` }));
      await user.keyboard("{Enter}");
      await act(async () => {
        await new Promise((r) => setTimeout(r, 0));
      });
      // Every drop target the drag offers, going down then up, as a screen reader user would.
      const targets = new Set<string>();
      for (const key of ["{ArrowDown}", "{ArrowUp}"]) {
        for (let i = 0; i < 12; i++) {
          const seen = label(document.activeElement);
          if (seen.startsWith("Insert")) targets.add(seen);
          await user.keyboard(key);
        }
      }
      return [...targets];
    }
    const label = (el: Element | null) => el?.getAttribute("aria-label") ?? "";

    async function dropAt(user: ReturnType<typeof userEvent.setup>, target: string) {
      for (let i = 0; i < 24 && label(document.activeElement) !== target; i++) {
        await user.keyboard(i < 12 ? "{ArrowDown}" : "{ArrowUp}");
      }
      expect(label(document.activeElement)).toBe(target);
      await user.keyboard("{Enter}");
      await act(async () => {
        await new Promise((r) => setTimeout(r, 0));
      });
    }

    it("offers an item every place but its own and sends one order request", async () => {
      const { props, user } = setup();
      const targets = await drag(user, "Phraseology");
      expect(targets).not.toContain("Insert before Phraseology");
      expect(targets).not.toContain("Insert between Phraseology and Readbacks");
      expect(targets).toContain("Insert after Taxi diagram");
      await dropAt(user, "Insert between Readbacks and Practice");
      expect(props.onReorderItems).toHaveBeenCalledTimes(1);
      expect(props.onReorderItems).toHaveBeenCalledWith("m1", ["i2", "i1", "i3"]);
      expect(await screen.findByRole("status")).toHaveTextContent(
        "Moved Phraseology to position 2 of 3 in Radio basics.",
      );
    });

    it("sends one move request for a drop in another module", async () => {
      const { props, user } = setup();
      await drag(user, "Phraseology");
      await dropAt(user, "Insert after Taxi diagram");
      expect(props.onMoveItem).toHaveBeenCalledTimes(1);
      expect(props.onMoveItem).toHaveBeenCalledWith("i1", { moduleId: "m2", position: 1 });
      expect(props.onReorderItems).not.toHaveBeenCalled();
    });

    it("offers a module only the places between modules", async () => {
      const { props, user } = setup();
      const targets = await drag(user, "2 Ground control");
      expect(targets.sort()).toEqual(["Insert before 1 Radio basics"]);
      await dropAt(user, "Insert before 1 Radio basics");
      expect(props.onReorderModules).toHaveBeenCalledWith(["m2", "m1"]);
    });
  });

  it("offers no Move up at the top of a module or Move down at the bottom", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Move Phraseology" }));
    expect(screen.getByRole("menuitem", { name: "Move up" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.getByRole("menuitem", { name: "Move down" })).not.toHaveAttribute(
      "aria-disabled",
    );
  });

  it("moves an item to the end of another module through the move route", async () => {
    const { props, user } = setup();
    await choose(user, "Move Readbacks", "Move to module", "2 Ground control");
    expect(props.onMoveItem).toHaveBeenCalledTimes(1);
    expect(props.onMoveItem).toHaveBeenCalledWith("i2", { moduleId: "m2", position: 1 });
    expect(props.onReorderItems).not.toHaveBeenCalled();
  });

  it("moves a module through the modules' order route", async () => {
    const { props, user } = setup();
    await choose(user, "Move 2 Ground control", "Move up");
    expect(props.onReorderModules).toHaveBeenCalledTimes(1);
    expect(props.onReorderModules).toHaveBeenCalledWith(["m2", "m1"]);
  });

  it("shows the move while it saves and refuses a second one meanwhile", async () => {
    let finish: (o: MoveOutcome) => void = () => {};
    const onReorderItems = vi.fn(() => new Promise<MoveOutcome>((r) => (finish = r)));
    const { user } = setup({ onReorderItems });
    await choose(user, "Move Phraseology", "Move down");
    expect(screen.getByRole("status")).toHaveTextContent("Saving the new order.");
    const order = () => screen.getAllByRole("row").map((r) => r.getAttribute("aria-label"));
    expect(order().slice(1, 3)).toEqual(["Readbacks", "Phraseology"]);
    await user.click(screen.getByRole("button", { name: "Move Readbacks" }));
    expect(screen.getByRole("menuitem", { name: "Move down" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    await act(async () => finish({ status: "moved" }));
    expect(onReorderItems).toHaveBeenCalledTimes(1);
  });

  it("on 409 order.stale reloads, says so, and keeps focus on the moved row", async () => {
    const { props, user, view } = setup({
      onReorderItems: vi.fn(async (): Promise<MoveOutcome> => ({ status: "stale" })),
    });
    await choose(user, "Move Phraseology", "Move down");
    expect(props.onReload).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("alert")).toHaveTextContent("The latest order is shown");
    // The reload brings another author's order, with the item now last.
    const [phraseology, ...others] = radio.items;
    const reloaded = [{ ...radio, items: [...others, ...radio.items.slice(0, 1)] }, ground];
    expect(phraseology?.title).toBe("Phraseology");
    (document.activeElement as HTMLElement | null)?.blur();
    view.rerender(<ModuleBoard {...props} modules={reloaded} />);
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Move Phraseology" })),
    );
    const order = screen.getAllByRole("row").map((r) => r.getAttribute("aria-label"));
    expect(order.slice(1, 4)).toEqual(["Readbacks", "Practice", "Phraseology"]);
  });

  it("puts the order back and shows the server's refusal", async () => {
    const { user } = setup({
      onReorderModules: vi.fn(
        async (): Promise<MoveOutcome> => ({
          status: "failed",
          message: "This course is archived.",
        }),
      ),
    });
    await choose(user, "Move 1 Radio basics", "Move down");
    expect(await screen.findByRole("alert")).toHaveTextContent("This course is archived.");
    expect(screen.getAllByRole("row")[0]).toHaveAttribute("aria-label", "1 Radio basics");
  });

  it("shows a write that throws as a failure, not a crash", async () => {
    const { user } = setup({ onReorderModules: vi.fn(() => Promise.reject(new Error("offline"))) });
    await choose(user, "Move 1 Radio basics", "Move down");
    expect(await screen.findByRole("alert")).toHaveTextContent("The move was not saved.");
  });

  it("makes no move when disabled", async () => {
    const { props, user } = setup({ isDisabled: true });
    expect(screen.getByRole("button", { name: "Drag Phraseology" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Move Phraseology" }));
    expect(screen.getByRole("menuitem", { name: "Move down" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(props.onReorderItems).not.toHaveBeenCalled();
  });

  it("opens an item's editor in a side sheet, uncontrolled", async () => {
    const { user } = setup({
      itemEditor: (item, { close }) => (
        <button type="button" onClick={close}>
          {`Done with ${item.title}`}
        </button>
      ),
    });
    await user.click(screen.getByRole("button", { name: "Edit Taxi diagram" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit Taxi diagram" });
    await user.click(within(sheet).getByRole("button", { name: "Done with Taxi diagram" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("opens the sheet the caller names, controlled", async () => {
    const onEditingItemIdChange = vi.fn();
    const { user } = setup({
      editingItemId: "i1",
      onEditingItemIdChange,
      itemEditor: () => <p>Editor</p>,
    });
    expect(screen.getByRole("dialog", { name: "Edit Phraseology" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(onEditingItemIdChange).toHaveBeenCalledWith(null);
    expect(screen.getByRole("dialog", { name: "Edit Phraseology" })).toBeInTheDocument();
  });

  it("shows the empty state for a course without modules", () => {
    setup({ modules: [], emptyState: "No modules yet." });
    expect(screen.getByText("No modules yet.")).toBeInTheDocument();
  });
});
