// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard's rows (docs/specs/m1/16-learning-ui-components.md B13): a module's number, title,
// publish state and unlock rules, or an item's kind, title, requirement and publish state, each
// with a drag handle and a Move menu, and for an item an Edit button. What a row draws is said
// again in one visually hidden description, so the drawn copy is hidden from screen readers.
import {
  DotsSixVerticalIcon,
  DotsThreeIcon,
  PencilSimpleIcon,
} from "@phosphor-icons/react";
import { useMemo } from "react";
import { Button as AriaButton, GridListItem, Text } from "react-aria-components";
import { tv } from "../lib/tv";
import { useUnitList } from "../lib/unit-list";
import { itemKindIcons } from "../learning/item-row";
import { useLockReasonText } from "../learning/lock-reason";
import { RequirementLabel, useRequirementText } from "../learning/requirement-label";
import { statusPill } from "../learning/status-pill";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { IconButton } from "../primitives/icon-button";
import { Menu, MenuItem, MenuTrigger, SubmenuTrigger } from "../primitives/menu";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import type { BoardModule, BoardRow } from "./board-moves";

export const moduleBoardRow = tv({
  slots: {
    root: [
      "flex flex-wrap items-center gap-x-3 gap-y-1 rounded-sm pe-1 font-ui text-sm text-fg-default",
      "outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken data-[dragging]:opacity-50",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
    drag: [
      "inline-flex size-(--sc-control-h) shrink-0 cursor-grab items-center justify-center rounded-sm",
      "text-fg-sub outline-none data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
    ],
    main: "flex min-h-(--sc-control-h) min-w-0 flex-[1_1_12rem] items-center gap-3 py-1.5",
    number: "shrink-0 text-fg-sub tabular-nums",
    title: "min-w-0 break-words",
    kindIcon: "shrink-0 text-fg-sub",
    // Indented past the handle, so a wrapped line sits under the title it belongs to.
    meta: "flex flex-wrap items-center gap-x-3 gap-y-1 ps-(--sc-control-h) sm:ps-0",
    pill: statusPill(),
    rules: "text-caption text-fg-sub break-words",
    actions: "ms-auto flex shrink-0 items-center",
  },
  variants: {
    type: {
      module: { root: "mt-4 border-t border-line pt-2 first:mt-0 first:border-t-0" },
      item: { root: "ps-6" },
      header: { root: "ps-6", title: "font-heading text-fg-strong" },
    },
  },
});

/** What the Move menu and the Edit button do; the board turns each into a write or the sheet. */
export interface RowActions {
  /** The Move menu: `up`, `down`, or `module:<id>` for "Move to module". */
  onMove: (key: string, action: string) => void;
  /** Opens the item in the editing sheet; none draws no Edit button. */
  onEdit?: (itemId: string) => void;
  /** No move while a move is saving or the board is disabled. */
  movesDisabled: boolean;
}

export interface BoardRowProps extends RowActions {
  row: BoardRow;
  modules: readonly BoardModule[];
  /** The row's accessible name, unique on the board. */
  name: string;
  timeZone: string;
  headingLevel: Exclude<HeadingLevel, 6>;
}

/** One row of the board's GridList. */
export function ModuleBoardRow(props: BoardRowProps) {
  const { row, name } = props;
  const type = row.type === "module" ? "module" : row.item.kind === "header" ? "header" : "item";
  const styles = moduleBoardRow({ type });
  const description = useRowDescription(row, props.modules, props.timeZone);
  return (
    <GridListItem
      id={row.key}
      data-key={row.key}
      data-slot={row.type === "module" ? "module-board-module" : "module-board-item"}
      data-kind={row.type === "item" ? row.item.kind : undefined}
      textValue={name}
      className={styles.root()}
    >
      <DragHandle name={name} className={styles.drag()} />
      {row.type === "module" ? (
        <ModuleMain {...props} row={row} styles={styles} />
      ) : (
        <ItemMain row={row} styles={styles} />
      )}
      <span data-slot="module-board-row-meta" aria-hidden="true" className={styles.meta()}>
        <RowMeta row={row} modules={props.modules} timeZone={props.timeZone} styles={styles} />
      </span>
      <Text slot="description" data-slot="module-board-row-description" className="sr-only">
        {description}
      </Text>
      <span data-slot="module-board-row-actions" className={styles.actions()}>
        {row.type === "item" && props.onEdit !== undefined ? (
          <EditButton itemId={row.item.id} name={name} onEdit={props.onEdit} />
        ) : null}
        <MoveMenu {...props} />
      </span>
    </GridListItem>
  );
}

type Styles = ReturnType<typeof moduleBoardRow>;

function DragHandle({ name, className }: { name: string; className: string }) {
  const { t } = useStepclimbStrings();
  return (
    <AriaButton
      slot="drag"
      data-slot="module-board-drag"
      aria-label={t("moduleBoard.drag", { name })}
      className={className}
    >
      <Icon icon={DotsSixVerticalIcon} size="md" />
    </AriaButton>
  );
}

function ModuleMain({
  row,
  headingLevel,
  styles,
}: BoardRowProps & { row: Extract<BoardRow, { type: "module" }>; styles: Styles }) {
  const { t } = useStepclimbStrings();
  return (
    <span data-slot="module-board-row-main" className={styles.main()}>
      <Heading level={headingLevel} size="lg" className="flex min-w-0 items-baseline gap-3">
        <span data-slot="module-board-number" className={styles.number()}>
          {t("moduleRow.number", { number: row.index + 1 })}
        </span>{" "}
        <span data-slot="module-board-title" className={styles.title()}>
          {row.module.title}
        </span>
      </Heading>
    </span>
  );
}

function ItemMain({
  row,
  styles,
}: {
  row: Extract<BoardRow, { type: "item" }>;
  styles: Styles;
}) {
  const { item } = row;
  return (
    <span data-slot="module-board-row-main" className={styles.main()}>
      {item.kind === "header" ? null : (
        <Icon icon={itemKindIcons[item.kind]} size="md" className={styles.kindIcon()} />
      )}
      <span data-slot="module-board-title" className={styles.title()}>
        {item.title}
      </span>
    </span>
  );
}

/** The drawn publish state, requirement and unlock rules; the description says them to a screen reader. */
function RowMeta({
  row,
  modules,
  timeZone,
  styles,
}: {
  row: BoardRow;
  modules: readonly BoardModule[];
  timeZone: string;
  styles: Styles;
}) {
  const { t } = useStepclimbStrings();
  const list = useUnitList();
  const rules = useModuleRules(row.type === "module" ? row.module : null, modules, timeZone);
  const published = row.type === "module" ? row.module.published : row.item.published;
  const requirement = row.type === "item" ? row.item.requirement : null;
  return (
    <>
      {requirement === null ? null : <RequirementLabel requirement={requirement} />}
      <span
        data-slot="module-board-published"
        data-published={published}
        className={styles.pill()}
      >
        {t("moduleBoard.published", { published: published ? "yes" : "no" })}
      </span>
      {rules.length === 0 ? null : (
        <span data-slot="module-board-rules" className={styles.rules()}>
          {list(rules)}
        </span>
      )}
    </>
  );
}

/**
 * A module's unlock rules as phrases, as the learner's lock reasons read ("Opens on Mar 3, 2026,
 * 10:00 AM EST", "Unlocks after Ground control"), then "Items in order"; "No items yet" for an
 * empty module.
 */
function useModuleRules(
  module: BoardModule | null,
  modules: readonly BoardModule[],
  timeZone: string,
): string[] {
  const { t } = useStepclimbStrings();
  const reasonText = useLockReasonText(timeZone);
  return useMemo(() => {
    if (module === null) return [];
    const rules: string[] = [];
    if (module.unlockAt !== null) rules.push(reasonText({ kind: "unlock_at", at: module.unlockAt }));
    // In course order, as the API lists them; an id the board does not hold names nothing.
    const titles = modules.filter((m) => module.prerequisiteIds.includes(m.id)).map((m) => m.title);
    const [first, ...rest] = titles;
    if (first !== undefined) {
      rules.push(reasonText({ kind: "prerequisites", moduleTitles: [first, ...rest] }));
    }
    if (module.requireSequential) rules.push(t("moduleBoard.sequential"));
    if (module.items.length === 0) rules.push(t("moduleBoard.noItems"));
    return rules;
  }, [module, modules, reasonText, t]);
}

/** Everything the row draws besides its name, as the locale lists units. */
function useRowDescription(
  row: BoardRow,
  modules: readonly BoardModule[],
  timeZone: string,
): string {
  const { t } = useStepclimbStrings();
  const list = useUnitList();
  const requirementText = useRequirementText();
  const rules = useModuleRules(row.type === "module" ? row.module : null, modules, timeZone);
  if (row.type === "module") {
    const published = row.module.published ? "yes" : "no";
    return list([t("moduleBoard.published", { published }), ...rules]);
  }
  const { item } = row;
  return list([
    item.kind === "header" ? t("moduleBoard.header") : t("itemRow.kind", { kind: item.kind }),
    ...(item.requirement === null ? [] : [requirementText(item.requirement)]),
    t("moduleBoard.published", { published: item.published ? "yes" : "no" }),
  ]);
}

function EditButton({
  itemId,
  name,
  onEdit,
}: {
  itemId: string;
  name: string;
  onEdit: (itemId: string) => void;
}) {
  const { t } = useStepclimbStrings();
  return (
    <IconButton
      data-slot="module-board-edit"
      icon={PencilSimpleIcon}
      label={t("moduleBoard.edit", { name })}
      onPress={() => onEdit(itemId)}
    />
  );
}

/** Up, down and, for an item, "Move to module": every move without a pointer. */
function MoveMenu({ row, modules, name, onMove, movesDisabled }: BoardRowProps) {
  const { t } = useStepclimbStrings();
  const disabled = movesDisabled ? ["up", "down", "module"] : edgeKeys(row, modules);
  const others = row.type === "item" ? modules.filter((m) => m.id !== row.module.id) : [];
  return (
    <MenuTrigger>
      <IconButton
        data-slot="module-board-move"
        icon={DotsThreeIcon}
        label={t("moduleBoard.move", { name })}
      />
      <Menu
        data-slot="module-board-move-menu"
        disabledKeys={disabled}
        onAction={(key) => onMove(row.key, String(key))}
      >
        <MenuItem id="up">{t("moduleBoard.moveUp")}</MenuItem>
        <MenuItem id="down">{t("moduleBoard.moveDown")}</MenuItem>
        {others.length === 0 ? null : (
          <SubmenuTrigger>
            <MenuItem id="module">{t("moduleBoard.moveToModule")}</MenuItem>
            <Menu
              data-slot="module-board-module-menu"
              disabledKeys={movesDisabled ? others.map((m) => `module:${m.id}`) : []}
              onAction={(key) => onMove(row.key, String(key))}
            >
              {others.map((m) => (
                <MenuItem key={m.id} id={`module:${m.id}`}>
                  {t("moduleBoard.moduleName", {
                    number: modules.indexOf(m) + 1,
                    title: m.title,
                  })}
                </MenuItem>
              ))}
            </Menu>
          </SubmenuTrigger>
        )}
      </Menu>
    </MenuTrigger>
  );
}

/** "Move up" at the top and "Move down" at the bottom of the module, or of the course, do nothing. */
function edgeKeys(row: BoardRow, modules: readonly BoardModule[]): string[] {
  const count = row.type === "module" ? modules.length : row.module.items.length;
  const keys: string[] = [];
  if (row.index === 0) keys.push("up");
  if (row.index === count - 1) keys.push("down");
  return keys;
}
