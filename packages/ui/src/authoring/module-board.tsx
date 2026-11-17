// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard: the course structure editor (docs/specs/m1/16-learning-ui-components.md B13). The
// modules and their items are one React Aria GridList, each module's row followed by its items'
// rows. A row moves by drag and drop, or from its Move menu (up, down, "Move to module"); either
// way the board makes one write of docs/specs/m1/03-modules-and-items.md B10 and B11 through the
// caller and shows the result while the server answers. On 409 `order.stale` it asks the caller to
// reload and keeps focus on the row that was moved. Item editing opens in a side sheet.
import { type ReactNode, type RefObject, useLayoutEffect, useMemo, useRef, useState } from "react";
import { DropIndicator, GridList, useDragAndDrop } from "react-aria-components";
import { tv } from "../lib/tv";
import { FormError } from "../patterns/form-error";
import type { HeadingLevel } from "../primitives/heading";
import { Sheet } from "../primitives/sheet";
import { useStepclimbStrings } from "../strings/strings";
import {
  applyMove,
  type BoardItem,
  type BoardModule,
  type BoardMove,
  type BoardRow,
  boardRows,
  dropMove,
  dropSpot,
  placeAfter,
  stepMove,
  toModuleMove,
} from "./board-moves";
import { ModuleBoardRow } from "./module-board-rows";

/**
 * What a write gave: `moved` once `modules` holds the server's answer, `stale` for 409
 * `order.stale`, `failed` with the message to show for any other refusal.
 */
export type MoveOutcome =
  | { status: "moved" }
  | { status: "stale" }
  | { status: "failed"; message: string };

const ROW_TYPE = "application/x-stepclimb-board-row";

export const moduleBoard = tv({
  slots: {
    root: "flex flex-col gap-3 font-ui",
    status: "min-h-5 text-sm text-fg-sub",
    list: "flex flex-col outline-none",
    dropIndicator: "h-0.5 rounded-pill data-[drop-target]:bg-brand",
    empty: "py-6 text-sm text-fg-sub",
  },
});

export interface ModuleBoardProps {
  /** The course's modules with their items, in order, as the server last sent them. */
  modules: readonly BoardModule[];
  /** `PUT /courses/{course_id}/modules/order` with every module id in the new order. */
  onReorderModules: (moduleIds: string[]) => Promise<MoveOutcome>;
  /** `PUT /modules/{module_id}/items/order` with every item id of the module in the new order. */
  onReorderItems: (moduleId: string, itemIds: string[]) => Promise<MoveOutcome>;
  /** `POST /items/{item_id}/move` to another module of the course, at `position` from 0. */
  onMoveItem: (itemId: string, to: { moduleId: string; position: number }) => Promise<MoveOutcome>;
  /** Fetches the modules again, after the server answered 409 `order.stale`. */
  onReload: () => void;
  /** The item editor shown in the side sheet; none draws no Edit buttons. */
  itemEditor?: (item: BoardItem, opts: { close: () => void }) => ReactNode;
  /** The item open in the sheet (controlled); null for none. */
  editingItemId?: string | null;
  /** The item open in the sheet at first (uncontrolled). */
  defaultEditingItemId?: string | null;
  onEditingItemIdChange?: (itemId: string | null) => void;
  /** No moves, as in an archived course, where every write answers 409. */
  isDisabled?: boolean;
  /** The user's IANA time zone (`users.tz`), for unlock dates. */
  timeZone: string;
  /** The modules' place in the page outline; 2 by default. */
  headingLevel?: Exclude<HeadingLevel, 6>;
  /** The list's name; "Modules and items" by default. */
  "aria-label"?: string;
  /** What shows when the course has no modules. */
  emptyState?: ReactNode;
  /** Layout only. */
  className?: string;
}

type Report =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "moved"; text: string }
  | { kind: "error"; text: string };

export function ModuleBoard(props: ModuleBoardProps) {
  const { t } = useStepclimbStrings();
  const styles = moduleBoard();
  const board = useBoardMoves(props);
  const { shown } = board;
  const rows = useMemo(() => boardRows(shown), [shown]);
  const names = useRowNames(rows, shown);
  const listRef = useRef<HTMLDivElement>(null);
  useRestoreFocus(listRef, board.restoreKey, shown);
  const editing = useEditing(props);
  const dragAndDropHooks = useBoardDrag(board, rows, props.isDisabled === true);
  const actions = {
    onMove: (key: string, action: string) => board.menuMove(key, action),
    onEdit: props.itemEditor === undefined ? undefined : editing.open,
    movesDisabled: props.isDisabled === true || board.pending,
  };
  const headingLevel = props.headingLevel ?? 2;
  return (
    <div data-slot="module-board" className={styles.root({ className: props.className })}>
      <BoardReport report={board.report} className={styles.status()} />
      <GridList
        ref={listRef}
        data-slot="module-board-list"
        aria-label={props["aria-label"] ?? t("moduleBoard.label")}
        data-pending={board.pending || undefined}
        items={rows}
        dependencies={[names, actions.movesDisabled, props.timeZone, headingLevel]}
        dragAndDropHooks={dragAndDropHooks}
        renderEmptyState={() => (
          <div data-slot="module-board-empty" className={styles.empty()}>
            {props.emptyState}
          </div>
        )}
        className={styles.list()}
      >
        {(row) => (
          <ModuleBoardRow
            {...actions}
            row={row}
            modules={shown}
            name={names.get(row.key) ?? ""}
            timeZone={props.timeZone}
            headingLevel={headingLevel}
          />
        )}
      </GridList>
      <EditSheet {...editing} modules={shown} itemEditor={props.itemEditor} />
    </div>
  );
}

/**
 * React Aria's drag and drop over the rows: every drop target that makes a write is offered, and
 * a drop runs that write. Off while a move saves or the board is disabled.
 */
function useBoardDrag(
  board: ReturnType<typeof useBoardMoves>,
  rows: readonly BoardRow[],
  isDisabled: boolean,
) {
  const { shown } = board;
  const styles = moduleBoard();
  const { dragAndDropHooks } = useDragAndDrop<BoardRow>({
    isDisabled: isDisabled || board.pending,
    getItems: (keys) => [...keys].map((key) => ({ [ROW_TYPE]: String(key) })),
    acceptedDragTypes: [ROW_TYPE],
    getDropOperation: (target, _types, _allowed) => {
      const dragged = board.dragging.current;
      if (target.type !== "item" || dragged === null) return "cancel";
      const spot = dropSpot(rows, target);
      return spot !== null && dropMove(shown, dragged, spot) !== null ? "move" : "cancel";
    },
    onDragStart: (e) => {
      board.dragging.current = String([...e.keys][0]);
    },
    onDragEnd: () => {
      board.dragging.current = null;
    },
    onReorder: (e) => {
      const key = String([...e.keys][0]);
      const spot = dropSpot(rows, e.target);
      const move = spot === null ? null : dropMove(shown, key, spot);
      if (move !== null) void board.run(key, move);
    },
    renderDropIndicator: (target) => (
      <DropIndicator
        target={target}
        data-slot="module-board-drop-indicator"
        className={styles.dropIndicator()}
      />
    ),
  });
  return dragAndDropHooks;
}

/**
 * The write in flight, the board shown meanwhile (the move applied to the server's last answer),
 * and the report of the last one.
 */
function useBoardMoves(props: ModuleBoardProps) {
  const { t } = useStepclimbStrings();
  const [optimistic, setOptimistic] = useState<BoardModule[] | null>(null);
  const [report, setReport] = useState<Report>({ kind: "idle" });
  const [restoreKey, setRestoreKey] = useState<string | null>(null);
  const dragging = useRef<string | null>(null);
  const shown = optimistic ?? props.modules;
  const pending = optimistic !== null;

  function send(move: BoardMove): Promise<MoveOutcome> {
    switch (move.kind) {
      case "modules":
        return props.onReorderModules(move.moduleIds);
      case "items":
        return props.onReorderItems(move.moduleId, move.itemIds);
      case "move":
        return props.onMoveItem(move.itemId, { moduleId: move.moduleId, position: move.position });
    }
  }

  async function run(key: string, move: BoardMove) {
    if (pending || props.isDisabled === true) return;
    const after = applyMove(shown, move);
    setOptimistic(after);
    setRestoreKey(key);
    setReport({ kind: "saving" });
    let outcome: MoveOutcome;
    try {
      outcome = await send(move);
    } catch {
      outcome = { status: "failed", message: t("moduleBoard.failed") };
    }
    setOptimistic(null);
    if (outcome.status === "moved") {
      setReport({ kind: "moved", text: movedText(t, after, key) });
    } else if (outcome.status === "stale") {
      setReport({ kind: "error", text: t("moduleBoard.stale") });
      props.onReload();
    } else {
      setReport({ kind: "error", text: outcome.message });
    }
  }

  function menuMove(key: string, action: string) {
    const move =
      action === "up" || action === "down"
        ? stepMove(shown, key, action === "up" ? -1 : 1)
        : action.startsWith("module:")
          ? toModuleMove(shown, key.replace(/^item:/, ""), action.slice("module:".length))
          : null;
    if (move !== null) void run(key, move);
  }

  return { shown, pending, report, restoreKey, dragging, run, menuMove };
}

function movedText(
  t: ReturnType<typeof useStepclimbStrings>["t"],
  after: BoardModule[],
  key: string,
) {
  const place = placeAfter(after, key);
  const row = boardRows(after).find((r) => r.key === key);
  if (place === null || row === undefined) return "";
  const title = row.type === "module" ? row.module.title : row.item.title;
  const values = { title, position: place.position, count: place.count };
  return place.module === null
    ? t("moduleBoard.movedModule", values)
    : t("moduleBoard.movedItem", { ...values, module: place.module.title });
}

/** Saving and moved in a status line; a refusal as an alert. */
function BoardReport({ report, className }: { report: Report; className: string }) {
  const { t } = useStepclimbStrings();
  return (
    <>
      <p data-slot="module-board-status" role="status" className={className}>
        {report.kind === "saving" ? t("moduleBoard.saving") : null}
        {report.kind === "moved" ? report.text : null}
      </p>
      {report.kind === "error" ? <FormError>{report.text}</FormError> : null}
    </>
  );
}

/**
 * Each row's name: a module's number and title, an item's title, or, where a title repeats on the
 * board, the title with its place, so no two rows, handles or buttons read the same.
 */
function useRowNames(rows: readonly BoardRow[], modules: readonly BoardModule[]) {
  const { t } = useStepclimbStrings();
  return useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of rows) {
      if (r.type === "item") counts.set(r.item.title, (counts.get(r.item.title) ?? 0) + 1);
    }
    const names = new Map<string, string>();
    for (const r of rows) {
      const number = modules.indexOf(r.module) + 1;
      if (r.type === "module") {
        names.set(r.key, t("moduleBoard.moduleName", { number, title: r.module.title }));
      } else if ((counts.get(r.item.title) ?? 0) > 1) {
        const values = { title: r.item.title, position: r.index + 1, number };
        names.set(r.key, t("moduleBoard.itemName", values));
      } else {
        names.set(r.key, r.item.title);
      }
    }
    return names;
  }, [rows, modules, t]);
}

/**
 * Keeps the place of the row last moved: when the move, its undoing or a reload re-renders the
 * list and focus falls to the page (its element was moved or replaced), focus goes back to the
 * row's Move button. Focus that is anywhere else stays where it is.
 */
function useRestoreFocus(
  listRef: RefObject<HTMLDivElement | null>,
  key: string | null,
  shown: readonly BoardModule[],
) {
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs after each change to the rows.
  useLayoutEffect(() => {
    if (key === null) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const row = listRef.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`);
    row?.querySelector<HTMLElement>("[data-slot=module-board-move]")?.focus();
  }, [key, shown, listRef]);
}

/** The item open in the sheet, controlled through `editingItemId` or kept here. */
function useEditing(props: ModuleBoardProps) {
  const [own, setOwn] = useState<string | null>(props.defaultEditingItemId ?? null);
  const controlled = props.editingItemId !== undefined;
  const itemId = controlled ? (props.editingItemId ?? null) : own;
  const set = (id: string | null) => {
    if (!controlled) setOwn(id);
    props.onEditingItemIdChange?.(id);
  };
  return { itemId, open: (id: string) => set(id), close: () => set(null) };
}

function EditSheet({
  itemId,
  close,
  modules,
  itemEditor,
}: {
  itemId: string | null;
  close: () => void;
  modules: readonly BoardModule[];
  itemEditor: ModuleBoardProps["itemEditor"];
}) {
  const { t } = useStepclimbStrings();
  const item = modules.flatMap((m) => m.items).find((i) => i.id === itemId);
  if (item === undefined || itemEditor === undefined) return null;
  return (
    <Sheet
      title={t("moduleBoard.editTitle", { title: item.title })}
      isOpen
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      {({ close: closeSheet }) => itemEditor(item, { close: closeSheet })}
    </Sheet>
  );
}
