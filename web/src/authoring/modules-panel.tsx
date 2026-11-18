// SPDX-License-Identifier: AGPL-3.0-or-later
// The modules panel of the course authoring screen (docs/specs/m1/16-learning-ui-components.md
// B13, B19): ModuleBoard over the course's modules, its three moves wired to spec 03's order and
// move routes, with the item and module editors in its side sheet, and the buttons that add a
// module or an item. Every write is followed by a read of the modules, so the board always draws
// the server's state; a move refused with 409 `order.stale` reads them again at once.
import { ListBulletsIcon } from "@phosphor-icons/react";
import {
  EmptyState,
  ModuleBoard,
  type MoveOutcome,
  Skeleton,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { LoadError } from "../layouts/load-error";
import { meQuery } from "../session";
import { AddItem, AddModule } from "./add-content";
import {
  boardModules,
  type Module,
  modulesKey,
  modulesQuery,
  moveItem,
  reorderItems,
  reorderModules,
} from "./course-content";
import { ItemEditor } from "./item-editor";
import { ModuleEditor } from "./module-editor";

interface ModulesPanelProps {
  courseId: string;
  /** False in an archived course, or without `course.content_edit`: the board is read-only. */
  canEdit: boolean;
}

/** The three moves, each followed by a read of the modules once the server stored it. */
function useMoves(courseId: string) {
  const { t } = useStepclimbStrings();
  const queryClient = useQueryClient();
  const settle = async (outcome: Promise<MoveOutcome>): Promise<MoveOutcome> => {
    const result = await outcome;
    if (result.status === "moved") {
      await queryClient.invalidateQueries({ queryKey: modulesKey(courseId) });
    }
    return result;
  };
  return {
    onReorderModules: (ids: string[]) => settle(reorderModules(courseId, ids, t)),
    onReorderItems: (moduleId: string, ids: string[]) => settle(reorderItems(moduleId, ids, t)),
    onMoveItem: (itemId: string, to: { moduleId: string; position: number }) =>
      settle(moveItem(itemId, to, t)),
    onReload: () => void queryClient.invalidateQueries({ queryKey: modulesKey(courseId) }),
  };
}

function findItem(modules: readonly Module[], itemId: string) {
  return modules.flatMap((m) => m.items).find((i) => i.id === itemId);
}

export function ModulesPanel({ courseId, canEdit }: ModulesPanelProps) {
  const { t } = useStepclimbStrings();
  const query = useQuery(modulesQuery(courseId));
  // The signed-in layout loaded `/me` before drawing this view; unlock dates are in its zone.
  const timeZone = useQuery(meQuery).data?.user.tz ?? "UTC";
  const moves = useMoves(courseId);
  // Mapped once per answer: the board tells a new answer from the one a move was made against.
  const board = useMemo(() => boardModules(query.data ?? []), [query.data]);
  if (query.isError && query.data === undefined) {
    return <LoadError error={query.error} level={2} onRetry={() => void query.refetch()} />;
  }
  if (query.data === undefined) {
    return (
      <div aria-busy="true" className="flex flex-col gap-3">
        <Skeleton className="h-11 w-48" />
        <Skeleton className="h-48 w-full" />
      </div>
    );
  }
  const modules = query.data;
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {canEdit ? (
        <div data-slot="authoring-toolbar" className="flex flex-wrap gap-2">
          <AddModule courseId={courseId} />
          <AddItem courseId={courseId} modules={board} />
        </div>
      ) : null}
      <ModuleBoard
        {...moves}
        modules={board}
        timeZone={timeZone}
        isDisabled={!canEdit}
        emptyState={
          <EmptyState icon={ListBulletsIcon}>
            {canEdit ? t("authoring.empty.edit") : t("authoring.empty")}
          </EmptyState>
        }
        itemEditor={(boardItem, { close }) => {
          const item = findItem(modules, boardItem.id);
          return item === undefined ? null : (
            <ItemEditor key={item.id} courseId={courseId} item={item} close={close} />
          );
        }}
        moduleEditor={(boardModule, { close }) => {
          const module = modules.find((m) => m.id === boardModule.id);
          return module === undefined ? null : (
            <ModuleEditor
              key={module.id}
              courseId={courseId}
              module={module}
              modules={board}
              timeZone={timeZone}
              close={close}
            />
          );
        }}
      />
    </div>
  );
}
