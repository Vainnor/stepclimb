// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard's moves as data (docs/specs/m1/16-learning-ui-components.md B13). The board draws
// the course as one list of rows, each module's row followed by its items' rows, so a drop lands
// between two rows. These functions turn a drop or a Move menu action into one of the three writes
// of docs/specs/m1/03-modules-and-items.md B10 and B11, and apply a write to the board's copy while
// the server answers. A move that would leave everything where it is gives no write.
import type { ItemKind } from "../learning/item-row";
import type { Requirement } from "../learning/requirement-label";

/** One item of a module, as the authoring routes return it (`Item`). */
export interface BoardItem {
  id: string;
  kind: ItemKind;
  title: string;
  published: boolean;
  /** What finishes the item for a learner; a header has none. */
  requirement: Requirement | null;
}

/** One module with its items in order, as `GET /courses/{id}/modules` returns it (`Module`). */
export interface BoardModule {
  id: string;
  title: string;
  published: boolean;
  /** `unlock_at`: before it, the module stays locked to learners. */
  unlockAt: Date | null;
  /** `prerequisite_ids`: the modules of the course this one waits for, in course order. */
  prerequisiteIds: readonly string[];
  /** `require_sequential`: whether the module's items go in order. */
  requireSequential: boolean;
  items: readonly BoardItem[];
}

/**
 * A write, one-to-one with spec 03's routes: `modules` is `PUT /courses/{id}/modules/order`,
 * `items` is `PUT /modules/{id}/items/order`, `move` is `POST /items/{id}/move`.
 */
export type BoardMove =
  | { kind: "modules"; moduleIds: string[] }
  | { kind: "items"; moduleId: string; itemIds: string[] }
  | { kind: "move"; itemId: string; moduleId: string; position: number };

/** A row of the board: a module, or an item of the module before it. */
export type BoardRow =
  | { type: "module"; key: string; module: BoardModule; index: number }
  | { type: "item"; key: string; item: BoardItem; module: BoardModule; index: number };

export const moduleKey = (id: string) => `module:${id}`;
export const itemKey = (id: string) => `item:${id}`;

/** The modules and items as one list of rows, in course order. */
export function boardRows(modules: readonly BoardModule[]): BoardRow[] {
  return modules.flatMap((module, index): BoardRow[] => [
    { type: "module", key: moduleKey(module.id), module, index },
    ...module.items.map(
      (item, i): BoardRow => ({ type: "item", key: itemKey(item.id), item, module, index: i }),
    ),
  ]);
}

/** Where a drop lands: before a row, or after the last one. */
export type DropSpot = { before: string } | "end";

/**
 * React Aria's drop target as a spot. "After a row" is the same place as "before the next one", so
 * every target reads as "before" except the end of the list. A drop on a row, not between two,
 * gives null: rows are not containers.
 */
export function dropSpot(
  rows: readonly BoardRow[],
  target: { key: string | number; dropPosition: "before" | "after" | "on" },
): DropSpot | null {
  const key = String(target.key);
  if (target.dropPosition === "on") return null;
  if (target.dropPosition === "before") return { before: key };
  const index = rows.findIndex((r) => r.key === key);
  if (index === -1) return null;
  const next = rows[index + 1];
  return next === undefined ? "end" : { before: next.key };
}

/** The write a drop of the row `dragged` at `spot` makes, or null when it is no move or not allowed. */
export function dropMove(
  modules: readonly BoardModule[],
  dragged: string,
  spot: DropSpot,
): BoardMove | null {
  const rows = boardRows(modules);
  const row = rows.find((r) => r.key === dragged);
  if (row === undefined) return null;
  return row.type === "module"
    ? moduleDrop(modules, row.module, rows, spot)
    : itemDrop(modules, row.item, row.module, rows, spot);
}

/** A module goes between two modules: before a module's row or at the end, never among items. */
function moduleDrop(
  modules: readonly BoardModule[],
  dragged: BoardModule,
  rows: readonly BoardRow[],
  spot: DropSpot,
): BoardMove | null {
  let at = modules.length;
  if (spot !== "end") {
    const target = rows.find((r) => r.key === spot.before);
    if (target?.type !== "module") return null;
    at = target.index;
  }
  return reorderModules(modules, dragged.id, at);
}

/**
 * An item goes before an item, or at the end of a module: a spot before a module's row is the end
 * of the module above it, and the end of the list is the end of the last module.
 */
function itemDrop(
  modules: readonly BoardModule[],
  dragged: BoardItem,
  from: BoardModule,
  rows: readonly BoardRow[],
  spot: DropSpot,
): BoardMove | null {
  let to: BoardModule | undefined;
  let position = 0;
  const target = spot === "end" ? undefined : rows.find((r) => r.key === spot.before);
  if (spot === "end") {
    to = modules.at(-1);
    position = to?.items.length ?? 0;
  } else if (target?.type === "item") {
    to = target.module;
    position = target.index;
  } else if (target?.type === "module") {
    to = modules[target.index - 1];
    position = to?.items.length ?? 0;
  }
  if (to === undefined) return null;
  if (to.id !== from.id) {
    return { kind: "move", itemId: dragged.id, moduleId: to.id, position };
  }
  const own = from.items.findIndex((i) => i.id === dragged.id);
  // Positions after the item's own place shift up by one once it is taken out.
  return reorderItems(from, dragged.id, position > own ? position - 1 : position);
}

/** Module `id` moved to `at` (0-based, among the others), or null when that is where it is. */
export function reorderModules(
  modules: readonly BoardModule[],
  id: string,
  at: number,
): BoardMove | null {
  const ids = modules.map((m) => m.id);
  const from = ids.indexOf(id);
  // A spot before a module counts that module's place with the dragged one still in the list.
  const to = at > from ? at - 1 : at;
  if (from === -1 || to === from) return null;
  ids.splice(from, 1);
  ids.splice(to, 0, id);
  return { kind: "modules", moduleIds: ids };
}

/** Item `id` moved to `at` in its own module (0-based, the item taken out), or null when it stays. */
export function reorderItems(module: BoardModule, id: string, at: number): BoardMove | null {
  const ids = module.items.map((i) => i.id);
  const from = ids.indexOf(id);
  if (from === -1 || at === from || at < 0 || at >= ids.length) return null;
  ids.splice(from, 1);
  ids.splice(at, 0, id);
  return { kind: "items", moduleId: module.id, itemIds: ids };
}

/** The Move menu's up and down: one place within the module, or the course for a module. */
export function stepMove(
  modules: readonly BoardModule[],
  key: string,
  step: -1 | 1,
): BoardMove | null {
  const row = boardRows(modules).find((r) => r.key === key);
  if (row === undefined) return null;
  if (row.type === "module") {
    const to = row.index + step;
    if (to < 0 || to >= modules.length) return null;
    // `reorderModules` counts a later place with the module still in the list.
    return reorderModules(modules, row.module.id, step === 1 ? to + 1 : to);
  }
  return reorderItems(row.module, row.item.id, row.index + step);
}

/** The Move menu's "Move to module": the end of another module. */
export function toModuleMove(
  modules: readonly BoardModule[],
  itemId: string,
  moduleId: string,
): BoardMove | null {
  const from = modules.find((m) => m.items.some((i) => i.id === itemId));
  const to = modules.find((m) => m.id === moduleId);
  if (from === undefined || to === undefined || from.id === to.id) return null;
  return { kind: "move", itemId, moduleId, position: to.items.length };
}

/** The board as it will be once the server applies `move`; what the board shows meanwhile. */
export function applyMove(modules: readonly BoardModule[], move: BoardMove): BoardModule[] {
  switch (move.kind) {
    case "modules":
      return move.moduleIds.flatMap((id) => modules.filter((m) => m.id === id));
    case "items":
      return modules.map((m) =>
        m.id === move.moduleId
          ? { ...m, items: move.itemIds.flatMap((id) => m.items.filter((i) => i.id === id)) }
          : m,
      );
    case "move": {
      const item = modules.flatMap((m) => m.items).find((i) => i.id === move.itemId);
      if (item === undefined) return [...modules];
      return modules.map((m) => {
        const items = m.items.filter((i) => i.id !== move.itemId);
        if (m.id === move.moduleId) items.splice(move.position, 0, item);
        return { ...m, items };
      });
    }
  }
}

/** Where `key` ends up after `move`: its 1-based place, the count beside it, and its module. */
export function placeAfter(
  modules: readonly BoardModule[],
  key: string,
): { position: number; count: number; module: BoardModule | null } | null {
  const row = boardRows(modules).find((r) => r.key === key);
  if (row === undefined) return null;
  if (row.type === "module") {
    return { position: row.index + 1, count: modules.length, module: null };
  }
  return { position: row.index + 1, count: row.module.items.length, module: row.module };
}
