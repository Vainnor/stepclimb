// SPDX-License-Identifier: AGPL-3.0-or-later
// ModuleBoard's rows (docs/specs/m1/16-learning-ui-components.md B13): a module's number, title,
// publish state and unlock rules, or an item's kind, title, requirement and publish state, each
// with a drag handle and a Move menu, and for an item an Edit button. What a row draws is said
// again in one visually hidden description, so the drawn copy is hidden from screen readers.
import {
  DotsSixVerticalIcon,
  DotsThreeIcon,
  EyeSlashIcon,
  PencilSimpleIcon,
} from "@phosphor-icons/react";
import { useMemo } from "react";
import {
  Button as AriaButton,
  GridListItem,
  Header,
  MenuSection,
  Text,
} from "react-aria-components";
import { itemKindIcons } from "../learning/item-row";
import { useLockReasonText } from "../learning/lock-reason";
import { RequirementLabel, useRequirementText } from "../learning/requirement-label";
import { statusPill } from "../learning/status-pill";
import { tv } from "../lib/tv";
import { useUnitList } from "../lib/unit-list";
import { useMediaQuery } from "../lib/use-media-query";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { IconButton } from "../primitives/icon-button";
import {
  MENU_POPOVER_QUERY,
  Menu,
  MenuItem,
  MenuTrigger,
  SubmenuTrigger,
} from "../primitives/menu";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import type { BoardModule, BoardRow } from "./board-moves";

export const moduleBoardRow = tv({
  slots: {
    root: [
      "flex items-start gap-x-2 rounded-sm pe-1 font-ui text-sm text-fg-default",
      "outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken data-[dragging]:opacity-50",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
    drag: [
      "inline-flex size-(--sc-control-h) shrink-0 cursor-grab items-center justify-center rounded-sm",
      "text-fg-sub outline-none data-[disabled]:cursor-not-allowed data-[disabled]:opacity-50",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
    ],
    // The title and the meta share a column between the handle and the buttons, so the meta wraps
    // under the title on a narrow screen and the buttons stay on the first line.
    body: "flex min-w-0 flex-1 flex-wrap items-center gap-x-4",
    // A single line sits in the middle of a control's height, level with the handle and buttons;
    // the kind icon stays on the first line when the title wraps.
    main: [
      "flex min-h-(--sc-control-h) min-w-0 flex-[1_1_12rem] items-start gap-3",
      "py-[calc((var(--sc-control-h)-1lh)/2)]",
    ],
    number: "shrink-0 text-fg-sub tabular-nums",
    title: "min-w-0 break-words",
    kindIcon: "flex h-lh shrink-0 items-center text-fg-sub",
    meta: "flex flex-wrap items-center gap-x-3 gap-y-1 pb-1.5 sm:py-1.5",
    pill: statusPill(),
    rules: "text-caption text-fg-sub break-words",
    actions: "flex shrink-0 items-center",
    menuName: "px-3 pt-2 pb-1 text-caption font-control text-fg-sub break-words",
  },
  variants: {
    type: {
      module: { root: "mt-4 border-t border-line pt-2 first:mt-0 first:border-t-0" },
      item: { root: "ps-6" },
      header: { root: "ps-6", title: "font-heading text-fg-strong" },
    },
    // Unpublished reads apart from Published without color: an icon, a dashed edge, a sunken fill.
    published: { true: {}, false: { pill: "gap-1 border-dashed bg-sunken" } },
  },
});

/** What the Move menu and the Edit button do; the board turns each into a write or the sheet. */
export interface RowActions {
  /** The Move menu: `up`, `down`, or `module:<id>` for "Move to module"; none draws no Move button. */
  onMove?: (key: string, action: string) => void;
  /** Opens the item in the editing sheet; none draws no Edit button. */
  onEdit?: (itemId: string) => void;
  /** No move while a move is saving. */
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
      <span data-slot="module-board-row-body" className={styles.body()}>
        {row.type === "module" ? (
          <ModuleMain {...props} row={row} styles={styles} />
        ) : (
          <ItemMain row={row} styles={styles} />
        )}
        <span data-slot="module-board-row-meta" aria-hidden="true" className={styles.meta()}>
          <RowMeta row={row} modules={props.modules} timeZone={props.timeZone} styles={styles} />
        </span>
      </span>
      <Text slot="description" data-slot="module-board-row-description" className="sr-only">
        {description}
      </Text>
      <span data-slot="module-board-row-actions" className={styles.actions()}>
        {row.type === "item" && props.onEdit !== undefined ? (
          <EditButton itemId={row.item.id} name={name} onEdit={props.onEdit} />
        ) : null}
        {props.onMove === undefined ? null : <MoveMenu {...props} onMove={props.onMove} />}
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

function ItemMain({ row, styles }: { row: Extract<BoardRow, { type: "item" }>; styles: Styles }) {
  const { item } = row;
  return (
    <span data-slot="module-board-row-main" className={styles.main()}>
      {item.kind === "header" ? null : (
        <span data-slot="module-board-kind" className={styles.kindIcon()}>
          <Icon icon={itemKindIcons[item.kind]} size="md" />
        </span>
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
        className={moduleBoardRow({ published }).pill()}
      >
        {published ? null : <Icon icon={EyeSlashIcon} size="sm" />}
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
    if (module.unlockAt !== null)
      rules.push(reasonText({ kind: "unlock_at", at: module.unlockAt }));
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
  const number = modules.indexOf(row.module) + 1;
  return list([
    item.kind === "header" ? t("moduleBoard.header") : t("itemRow.kind", { kind: item.kind }),
    ...(item.requirement === null ? [] : [requirementText(item.requirement)]),
    t("moduleBoard.published", { published: item.published ? "yes" : "no" }),
    t("moduleBoard.inModule", { number, title: row.module.title }),
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
function MoveMenu({
  row,
  modules,
  name,
  onMove,
  movesDisabled,
}: BoardRowProps & { onMove: (key: string, action: string) => void }) {
  const { t } = useStepclimbStrings();
  const styles = moduleBoardRow();
  // A bottom sheet has no trigger beside it, so it says which row it moves.
  const sheet = !useMediaQuery(MENU_POPOVER_QUERY);
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
        <MenuSection data-slot="module-board-move-section">
          {sheet ? (
            <Header data-slot="module-board-move-name" className={styles.menuName()}>
              {name}
            </Header>
          ) : null}
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
        </MenuSection>
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
