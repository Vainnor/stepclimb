// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard: the course structure editor (docs/specs/m1/16-learning-ui-components.md B13). The
// modules and their items are one React Aria GridList, each module's row followed by its items'
// rows. A row moves by drag and drop, or from its Move menu (up, down, "Move to module"); either
// way the board makes one write of docs/specs/m1/03-modules-and-items.md B10 and B11 through the
// caller and shows the result while the server answers. On 409 `order.stale` it asks the caller to
// reload and keeps focus on the row that was moved. Item and module editing open in a side sheet.
// A row deleted from its editor takes focus with it to the row now in its place: React Aria's
// collection moves its focused key to a neighbor when that key leaves.
import { type ReactNode, useMemo, useRef, useState } from "react";
import { DropIndicator, GridList, useDragAndDrop } from "react-aria-components";
import { tv } from "../lib/tv";
import { FormError } from "../patterns/form-error";
import type { HeadingLevel } from "../primitives/heading";
import { Sheet } from "../primitives/sheet";
import { useStepclimbStrings } from "../strings/strings";
import {
  type BoardItem,
  type BoardModule,
  type BoardRow,
  boardRows,
  dropMove,
  dropSpot,
} from "./board-moves";
import { ModuleBoardRow } from "./module-board-rows";
import { type MoveOutcome, type Report, useBoardMoves } from "./use-board-moves";

export type { MoveOutcome };

const ROW_TYPE = "application/x-stepclimb-board-row";

export const moduleBoard = tv({
  slots: {
    root: "flex flex-col gap-3 font-ui",
    status: "min-h-5 text-sm text-fg-sub",
    list: "flex flex-col outline-none",
    dropIndicator: "h-0.5 rounded-xs data-[drop-target]:bg-brand",
    empty: "py-6 text-sm text-fg-sub",
  },
  variants: {
    /** An item's drop line starts where item rows do, so it reads as joining the module above. */
    draggingItem: { true: { dropIndicator: "ms-6" } },
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
  /** The item editor shown in the side sheet; none draws no Edit buttons on items. */
  itemEditor?: (item: BoardItem, opts: { close: () => void }) => ReactNode;
  /** The item open in the sheet (controlled); null for none. */
  editingItemId?: string | null;
  /** The item open in the sheet at first (uncontrolled). */
  defaultEditingItemId?: string | null;
  onEditingItemIdChange?: (itemId: string | null) => void;
  /**
   * The module editor shown in the side sheet (its title, publish state and rules); none draws no
   * Edit buttons on modules.
   */
  moduleEditor?: (module: BoardModule, opts: { close: () => void }) => ReactNode;
  /** The module open in the sheet (controlled); null for none. */
  editingModuleId?: string | null;
  /** The module open in the sheet at first (uncontrolled). */
  defaultEditingModuleId?: string | null;
  onEditingModuleIdChange?: (moduleId: string | null) => void;
  /**
   * Read-only, as in an archived course, where every write answers 409: no drag handles, no Move
   * or Edit buttons and no editing sheet.
   */
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

export function ModuleBoard(props: ModuleBoardProps) {
  const { t } = useStepclimbStrings();
  const styles = moduleBoard();
  const listRef = useRef<HTMLDivElement>(null);
  const board = useBoardMoves(props, listRef);
  const { shown } = board;
  const rows = useMemo(() => boardRows(shown), [shown]);
  const names = useRowNames(rows, shown);
  const editingItem = useEditing(
    props.editingItemId,
    props.defaultEditingItemId,
    props.onEditingItemIdChange,
  );
  const editingModule = useEditing(
    props.editingModuleId,
    props.defaultEditingModuleId,
    props.onEditingModuleIdChange,
  );
  const readOnly = props.isDisabled === true;
  const dragAndDropHooks = useBoardDrag(board, rows, readOnly);
  const actions = {
    onMove: readOnly ? undefined : (key: string, action: string) => board.menuMove(key, action),
    onEditItem:
      props.itemEditor === undefined || readOnly
        ? undefined
        : (key: string) => editingItem.open(key.slice("item:".length)),
    onEditModule:
      props.moduleEditor === undefined || readOnly
        ? undefined
        : (key: string) => editingModule.open(key.slice("module:".length)),
    movesDisabled: readOnly || board.pending,
    readOnly,
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
        dependencies={[names, actions.movesDisabled, readOnly, props.timeZone, headingLevel]}
        dragAndDropHooks={readOnly ? undefined : dragAndDropHooks}
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
      {readOnly ? null : (
        <EditSheet
          item={editingItem}
          module={editingModule}
          modules={shown}
          itemEditor={props.itemEditor}
          moduleEditor={props.moduleEditor}
        />
      )}
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
  const dragging = useRef<string | null>(null);
  const { dragAndDropHooks } = useDragAndDrop<BoardRow>({
    isDisabled: isDisabled || board.pending,
    getItems: (keys) => [...keys].map((key) => ({ [ROW_TYPE]: String(key) })),
    acceptedDragTypes: [ROW_TYPE],
    getDropOperation: (target, _types, _allowed) => {
      const dragged = dragging.current;
      if (target.type !== "item" || dragged === null) return "cancel";
      const spot = dropSpot(rows, target);
      return spot !== null && dropMove(shown, dragged, spot) !== null ? "move" : "cancel";
    },
    onDragStart: (e) => {
      dragging.current = String([...e.keys][0]);
    },
    onDragEnd: () => {
      dragging.current = null;
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
        className={moduleBoard({
          draggingItem: dragging.current?.startsWith("item:") ?? false,
        }).dropIndicator()}
      />
    ),
  });
  return dragAndDropHooks;
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

/** The item or module open in the sheet, controlled through its id or kept here. */
function useEditing(
  controlledId: string | null | undefined,
  defaultId: string | null | undefined,
  onChange: ((id: string | null) => void) | undefined,
) {
  const [own, setOwn] = useState<string | null>(defaultId ?? null);
  const controlled = controlledId !== undefined;
  const id = controlled ? (controlledId ?? null) : own;
  const set = (next: string | null) => {
    if (!controlled) setOwn(next);
    onChange?.(next);
  };
  return { id, open: (next: string) => set(next), close: () => set(null) };
}

type Editing = ReturnType<typeof useEditing>;

function EditSheet({
  item,
  module,
  modules,
  itemEditor,
  moduleEditor,
}: {
  item: Editing;
  module: Editing;
  modules: readonly BoardModule[];
  itemEditor: ModuleBoardProps["itemEditor"];
  moduleEditor: ModuleBoardProps["moduleEditor"];
}) {
  const { t } = useStepclimbStrings();
  const editedItem = modules.flatMap((m) => m.items).find((i) => i.id === item.id);
  const editedModule = modules.find((m) => m.id === module.id);
  const target =
    editedItem !== undefined && itemEditor !== undefined
      ? {
          title: editedItem.title,
          close: item.close,
          body: (close: () => void) => itemEditor(editedItem, { close }),
        }
      : editedModule !== undefined && moduleEditor !== undefined
        ? {
            title: editedModule.title,
            close: module.close,
            body: (close: () => void) => moduleEditor(editedModule, { close }),
          }
        : null;
  if (target === null) return null;
  return (
    <Sheet
      title={t("moduleBoard.editTitle", { title: target.title })}
      isOpen
      onOpenChange={(open) => {
        if (!open) target.close();
      }}
    >
      {({ close: closeSheet }) => target.body(closeSheet)}
    </Sheet>
  );
}
