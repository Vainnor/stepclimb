// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  applyMove,
  type BoardItem,
  type BoardModule,
  boardRows,
  dropMove,
  dropSpot,
  itemKey,
  moduleKey,
  stepMove,
  toModuleMove,
} from "./board-moves";

const item = (id: string): BoardItem => ({
  id,
  kind: "page",
  title: id.toUpperCase(),
  published: true,
  requirement: null,
});

const mod = (id: string, items: string[]): BoardModule => ({
  id,
  title: id,
  published: true,
  unlockAt: null,
  prerequisiteIds: [],
  requireSequential: false,
  items: items.map(item),
});

// Rows: module:a, item:a1, item:a2, item:a3, module:b, item:b1, module:c (empty).
const modules = [mod("a", ["a1", "a2", "a3"]), mod("b", ["b1"]), mod("c", [])];
const rows = boardRows(modules);

describe("dropSpot", () => {
  it("reads after a row as before the next, and after the last as the end", () => {
    expect(dropSpot(rows, { key: itemKey("a3"), dropPosition: "after" })).toEqual({
      before: moduleKey("b"),
    });
    expect(dropSpot(rows, { key: moduleKey("c"), dropPosition: "after" })).toBe("end");
    expect(dropSpot(rows, { key: itemKey("a1"), dropPosition: "before" })).toEqual({
      before: itemKey("a1"),
    });
  });

  it("refuses a drop on a row", () => {
    expect(dropSpot(rows, { key: moduleKey("b"), dropPosition: "on" })).toBeNull();
  });
});

describe("dropMove for an item", () => {
  it("reorders its own module through the order route, counting the item taken out", () => {
    expect(dropMove(modules, itemKey("a1"), { before: itemKey("a3") })).toEqual({
      kind: "items",
      moduleId: "a",
      itemIds: ["a2", "a1", "a3"],
    });
    expect(dropMove(modules, itemKey("a3"), { before: itemKey("a1") })).toEqual({
      kind: "items",
      moduleId: "a",
      itemIds: ["a3", "a1", "a2"],
    });
  });

  it("puts a drop before a module's row at the end of the module above", () => {
    expect(dropMove(modules, itemKey("a1"), { before: moduleKey("b") })).toEqual({
      kind: "items",
      moduleId: "a",
      itemIds: ["a2", "a3", "a1"],
    });
    expect(dropMove(modules, itemKey("a1"), { before: moduleKey("c") })).toEqual({
      kind: "move",
      itemId: "a1",
      moduleId: "b",
      position: 1,
    });
  });

  it("moves to another module through the move route, into an empty one at the end", () => {
    expect(dropMove(modules, itemKey("b1"), { before: itemKey("a2") })).toEqual({
      kind: "move",
      itemId: "b1",
      moduleId: "a",
      position: 1,
    });
    expect(dropMove(modules, itemKey("b1"), "end")).toEqual({
      kind: "move",
      itemId: "b1",
      moduleId: "c",
      position: 0,
    });
  });

  it("gives no write for its own place or before the first module", () => {
    expect(dropMove(modules, itemKey("a2"), { before: itemKey("a2") })).toBeNull();
    expect(dropMove(modules, itemKey("a2"), { before: itemKey("a3") })).toBeNull();
    expect(dropMove(modules, itemKey("a3"), { before: moduleKey("b") })).toBeNull();
    expect(dropMove(modules, itemKey("a1"), { before: moduleKey("a") })).toBeNull();
  });
});

describe("dropMove for a module", () => {
  it("lands only between modules", () => {
    expect(dropMove(modules, moduleKey("c"), { before: moduleKey("a") })).toEqual({
      kind: "modules",
      moduleIds: ["c", "a", "b"],
    });
    expect(dropMove(modules, moduleKey("a"), "end")).toEqual({
      kind: "modules",
      moduleIds: ["b", "c", "a"],
    });
    expect(dropMove(modules, moduleKey("a"), { before: moduleKey("c") })).toEqual({
      kind: "modules",
      moduleIds: ["b", "a", "c"],
    });
    expect(dropMove(modules, moduleKey("c"), { before: itemKey("a2") })).toBeNull();
  });

  it("gives no write for its own place", () => {
    expect(dropMove(modules, moduleKey("a"), { before: moduleKey("a") })).toBeNull();
    expect(dropMove(modules, moduleKey("a"), { before: moduleKey("b") })).toBeNull();
    expect(dropMove(modules, moduleKey("c"), "end")).toBeNull();
  });
});

describe("the Move menu's moves", () => {
  it("steps an item within its module and stops at the ends", () => {
    expect(stepMove(modules, itemKey("a2"), -1)).toEqual({
      kind: "items",
      moduleId: "a",
      itemIds: ["a2", "a1", "a3"],
    });
    expect(stepMove(modules, itemKey("a2"), 1)).toEqual({
      kind: "items",
      moduleId: "a",
      itemIds: ["a1", "a3", "a2"],
    });
    expect(stepMove(modules, itemKey("a1"), -1)).toBeNull();
    expect(stepMove(modules, itemKey("b1"), 1)).toBeNull();
  });

  it("steps a module within the course", () => {
    expect(stepMove(modules, moduleKey("a"), 1)).toEqual({
      kind: "modules",
      moduleIds: ["b", "a", "c"],
    });
    expect(stepMove(modules, moduleKey("c"), -1)).toEqual({
      kind: "modules",
      moduleIds: ["a", "c", "b"],
    });
    expect(stepMove(modules, moduleKey("a"), -1)).toBeNull();
    expect(stepMove(modules, moduleKey("c"), 1)).toBeNull();
  });

  it("moves an item to the end of another module", () => {
    expect(toModuleMove(modules, "a2", "b")).toEqual({
      kind: "move",
      itemId: "a2",
      moduleId: "b",
      position: 1,
    });
    expect(toModuleMove(modules, "a2", "a")).toBeNull();
  });
});

describe("applyMove", () => {
  it("draws each write as the server will store it", () => {
    const ids = (ms: BoardModule[]) => ms.map((m) => [m.id, m.items.map((i) => i.id)]);
    expect(ids(applyMove(modules, { kind: "modules", moduleIds: ["c", "a", "b"] }))).toEqual([
      ["c", []],
      ["a", ["a1", "a2", "a3"]],
      ["b", ["b1"]],
    ]);
    expect(
      ids(applyMove(modules, { kind: "items", moduleId: "a", itemIds: ["a3", "a1", "a2"] })),
    ).toEqual([
      ["a", ["a3", "a1", "a2"]],
      ["b", ["b1"]],
      ["c", []],
    ]);
    expect(
      ids(applyMove(modules, { kind: "move", itemId: "a2", moduleId: "b", position: 0 })),
    ).toEqual([
      ["a", ["a1", "a3"]],
      ["b", ["a2", "b1"]],
      ["c", []],
    ]);
  });
});
