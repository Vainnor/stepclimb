// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard's move in flight (docs/specs/m1/16-learning-ui-components.md B13, B13a). A move is
// drawn at once and stays drawn until the caller's `modules` change, so the old order never shows
// again in between; the "Moved ..." report is read from those new modules, the server's order.
// Focus goes back to the moved row only on the render the move, its undoing or its reload makes,
// and only when it fell to the page; a later change to `modules` leaves focus alone.
import { type RefObject, useLayoutEffect, useRef, useState } from "react";
import { useStepclimbStrings } from "../strings/strings";
import {
  applyMove,
  type BoardModule,
  type BoardMove,
  boardRows,
  placeAfter,
  stepMove,
  toModuleMove,
} from "./board-moves";

/**
 * What a write gave: `moved` once the server stored it (the caller then passes the new
 * `modules`), `stale` for 409 `order.stale`, `failed` with the message to show for any other
 * refusal.
 */
export type MoveOutcome =
  | { status: "moved" }
  | { status: "stale" }
  | { status: "failed"; message: string };

export interface MoveHandlers {
  modules: readonly BoardModule[];
  onReorderModules: (moduleIds: string[]) => Promise<MoveOutcome>;
  onReorderItems: (moduleId: string, itemIds: string[]) => Promise<MoveOutcome>;
  onMoveItem: (itemId: string, to: { moduleId: string; position: number }) => Promise<MoveOutcome>;
  onReload: () => void;
  isDisabled?: boolean;
}

export type Report =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "moved"; text: string }
  | { kind: "error"; text: string };

/**
 * A move from the moment it is made until the caller's `modules` change: `saving` while the
 * request runs, `moved` and `stale` while the new or reloaded modules are on their way. `base` is
 * the `modules` it was made against.
 */
type Flight = {
  key: string;
  base: readonly BoardModule[];
  proposal: BoardModule[];
  phase: "saving" | "moved" | "stale";
};

type T = ReturnType<typeof useStepclimbStrings>["t"];

export function useBoardMoves(props: MoveHandlers, listRef: RefObject<HTMLDivElement | null>) {
  const { t } = useStepclimbStrings();
  const [flight, setFlight] = useState<Flight | null>(null);
  const [report, setReport] = useState<Report>({ kind: "idle" });
  // Refs, not state: two moves in one tick both read `false` from state, but not from the ref.
  const busy = useRef(false);
  const latest = useRef(props.modules);
  const focusAfterRender = useRef<string | null>(null);
  const showsProposal =
    flight !== null && flight.phase !== "stale" && props.modules === flight.base;
  const shown = showsProposal ? flight.proposal : props.modules;

  useLayoutEffect(() => {
    latest.current = props.modules;
  });
  // The render a move, its undoing or its settling made: put focus back, once.
  useLayoutEffect(() => {
    const key = focusAfterRender.current;
    focusAfterRender.current = null;
    if (key !== null) restoreFocus(listRef, key);
  });
  // The caller's new modules after a save or a reload: the move is over.
  useLayoutEffect(() => {
    if (flight === null || flight.phase === "saving" || props.modules === flight.base) return;
    if (flight.phase === "moved") setReport(movedReport(t, props.modules, flight.key));
    focusAfterRender.current = flight.key;
    setFlight(null);
  }, [props.modules, flight, t]);

  async function run(key: string, move: BoardMove) {
    if (busy.current || props.isDisabled === true) return;
    busy.current = true;
    const base = props.modules;
    focusAfterRender.current = key;
    setFlight({ key, base, proposal: applyMove(shown, move), phase: "saving" });
    setReport({ kind: "saving" });
    const outcome = await send(props, move).catch(
      (): MoveOutcome => ({ status: "failed", message: t("moduleBoard.failed") }),
    );
    busy.current = false;
    settle(key, base, outcome);
  }

  function settle(key: string, base: readonly BoardModule[], outcome: MoveOutcome) {
    if (outcome.status === "failed") {
      focusAfterRender.current = key;
      setFlight(null);
      setReport({ kind: "error", text: outcome.message });
    } else if (outcome.status === "stale") {
      setFlight((f) => (f === null ? null : { ...f, phase: "stale" }));
      setReport({ kind: "error", text: t("moduleBoard.stale") });
      props.onReload();
    } else if (latest.current !== base) {
      // The caller passed the new modules before it answered.
      focusAfterRender.current = key;
      setFlight(null);
      setReport(movedReport(t, latest.current, key));
    } else {
      setFlight((f) => (f === null ? null : { ...f, phase: "moved" }));
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

  return { shown, pending: flight?.phase === "saving", report, run, menuMove };
}

function send(props: MoveHandlers, move: BoardMove): Promise<MoveOutcome> {
  switch (move.kind) {
    case "modules":
      return props.onReorderModules(move.moduleIds);
    case "items":
      return props.onReorderItems(move.moduleId, move.itemIds);
    case "move":
      return props.onMoveItem(move.itemId, { moduleId: move.moduleId, position: move.position });
  }
}

/** "Moved Readbacks to position 1 of 2 in Ground control.", from the server's order. */
function movedReport(t: T, modules: readonly BoardModule[], key: string): Report {
  const place = placeAfter(modules, key);
  const row = boardRows(modules).find((r) => r.key === key);
  if (place === null || row === undefined) return { kind: "idle" };
  const title = row.type === "module" ? row.module.title : row.item.title;
  const values = { title, position: place.position, count: place.count };
  const text =
    place.module === null
      ? t("moduleBoard.movedModule", values)
      : t("moduleBoard.movedItem", { ...values, module: place.module.title });
  return { kind: "moved", text };
}

/** Focus back on the row's Move button, only when it fell to the page. */
function restoreFocus(listRef: RefObject<HTMLDivElement | null>, key: string) {
  const active = document.activeElement;
  if (active !== null && active !== document.body) return;
  const row = listRef.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`);
  row?.querySelector<HTMLElement>("[data-slot=module-board-move]")?.focus();
}
