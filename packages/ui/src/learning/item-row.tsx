// SPDX-License-Identifier: AGPL-3.0-or-later
// ItemRow: one item of a module on the course home, with its kind's icon, its title, what
// finishes it, its learning state and when it is due (docs/specs/m1/16-learning-ui-components.md
// B9). The title is the link to the item view, named by the title alone so each link on the page
// says which item it opens; the link's hit area covers the whole row. The kind, requirement, state
// and due label, listed in one visually hidden text, are its description, and the drawn ones are
// hidden from screen readers so they are not read twice. A locked item stays a link: the item
// view says why it is locked (docs/specs/m1/04-unlock-and-completion-rules.md B13). A header item
// is a heading that groups the items below it (docs/specs/m1/03-modules-and-items.md B8).
import {
  ArrowSquareOutIcon,
  FileTextIcon,
  ListChecksIcon,
  PaperclipIcon,
  type Icon as PhosphorIcon,
  PuzzlePieceIcon,
  VideoIcon,
} from "@phosphor-icons/react";
import { useId } from "react";
import { Link as AriaLink } from "react-aria-components";
import { tv } from "../lib/tv";
import { useUnitList } from "../lib/unit-list";
import { Heading, type HeadingLevel } from "../primitives/heading";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { DueBadge, useDueLabel } from "./due-badge";
import { type Requirement, RequirementLabel, useRequirementText } from "./requirement-label";
import { type LearningState, StatusChip } from "./status-chip";

/** The API's `ItemKind`. */
export const itemKinds = [
  "header",
  "page",
  "file",
  "external_link",
  "video",
  "quiz",
  "activity",
] as const;
export type ItemKind = (typeof itemKinds)[number];

/** Each kind's icon. A header has none: it is a heading, not an item to open. */
export const itemKindIcons: Record<Exclude<ItemKind, "header">, PhosphorIcon> = {
  page: FileTextIcon,
  file: PaperclipIcon,
  external_link: ArrowSquareOutIcon,
  video: VideoIcon,
  quiz: ListChecksIcon,
  activity: PuzzlePieceIcon,
};

/** One item, in the learning state the server reports for the learner. */
export type ItemRowItem = {
  id: string;
  /** The item's title, as the learner sees it. */
  title: string;
} & (
  | { kind: "header" }
  | {
      kind: Exclude<ItemKind, "header">;
      /** The item view's path. */
      href: string;
      state: LearningState;
      /** What finishes the item; none shows no label. */
      requirement?: Requirement | null;
      /** When the item is due for the learner, if it is. */
      due?: Date;
    }
);

export const itemRow = tv({
  slots: {
    root: [
      "relative flex flex-wrap items-center gap-x-4 gap-y-1 rounded-sm ps-2 pe-3 font-ui text-sm",
      "transition-colors duration-fast ease-out-expo",
      "has-[[data-hovered]]:bg-sunken has-[[data-pressed]]:bg-sunken",
      "has-[[data-focus-visible]]:ring-2 has-[[data-focus-visible]]:ring-inset has-[[data-focus-visible]]:ring-ring",
    ],
    link: [
      "flex min-h-(--sc-control-h) min-w-0 flex-[1_1_14rem] cursor-pointer items-center gap-3 py-1.5",
      // The row draws the focus ring, so the link drops base.css's own.
      "pointer-coarse:min-h-(--sc-control-touch) text-fg-default outline-none shadow-none",
      // The whole row is the target: the pseudo-element covers it, over the description.
      "after:absolute after:inset-0 after:rounded-sm after:content-['']",
    ],
    kindIcon: "text-fg-sub",
    title: "min-w-0 break-words",
    // Indented past the kind icon, so a wrapped line sits under the title it belongs to.
    meta: "flex flex-wrap items-center gap-x-3 gap-y-1 ps-8 pb-1.5 sm:py-1.5",
    header: "list-none ps-2 pe-3 pt-4 pb-1",
  },
  variants: {
    locked: { true: { title: "text-fg-soft" } },
  },
});

export interface ItemRowProps {
  item: ItemRowItem;
  /** The user's IANA time zone (`users.tz`), for the due label. */
  timeZone: string;
  /** The current instant, from the app's clock, for the due label. */
  now: Date;
  /** A header item's place in the page outline: one below its module's heading; 3 by default. */
  headingLevel?: HeadingLevel;
  /** Layout only. */
  className?: string;
}

/** A list item: render it inside an `ol` or `ul`, as ModuleRow does. */
export function ItemRow({ item, timeZone, now, headingLevel = 3, className }: ItemRowProps) {
  const styles = itemRow({ locked: item.kind !== "header" && item.state === "locked" });
  if (item.kind === "header") {
    return (
      <li data-slot="item-row" data-kind="header" className={styles.header({ className })}>
        <Heading level={headingLevel} size="base">
          {item.title}
        </Heading>
      </li>
    );
  }
  return (
    <li
      data-slot="item-row"
      data-kind={item.kind}
      data-state={item.state}
      className={styles.root({ className })}
    >
      <ItemLink item={item} timeZone={timeZone} now={now} />
    </li>
  );
}

type OpenableItem = Exclude<ItemRowItem, { kind: "header" }>;

function ItemLink({ item, timeZone, now }: { item: OpenableItem; timeZone: string; now: Date }) {
  const description = useItemDescription(item, timeZone, now);
  const descriptionId = useId();
  const requirement = item.requirement ?? null;
  const styles = itemRow({ locked: item.state === "locked" });
  return (
    <>
      <AriaLink
        data-slot="item-row-link"
        href={item.href}
        aria-describedby={descriptionId}
        className={styles.link()}
      >
        <Icon icon={itemKindIcons[item.kind]} size="md" className={styles.kindIcon()} />
        <span data-slot="item-row-title" className={styles.title()}>
          {item.title}
        </span>
      </AriaLink>
      <VisuallyHidden id={descriptionId} data-slot="item-row-description">
        {description}
      </VisuallyHidden>
      {/* The link's description says all of this, so a screen reader does not read it twice. */}
      <span data-slot="item-row-meta" aria-hidden="true" className={styles.meta()}>
        {requirement === null ? null : <RequirementLabel requirement={requirement} />}
        <StatusChip state={item.state} />
        {item.due === undefined ? null : <DueBadge due={item.due} now={now} timeZone={timeZone} />}
      </span>
    </>
  );
}

/** The kind, requirement, state and due label as one list ("Page, Mark as done, Available"). */
function useItemDescription(item: OpenableItem, timeZone: string, now: Date): string {
  const { t } = useStepclimbStrings();
  const requirementText = useRequirementText();
  const list = useUnitList();
  // Called for every item, so the hooks run in the same order; only an item with a due date uses it.
  const due = useDueLabel(item.due ?? now, now, timeZone);
  const requirement = item.requirement ?? null;
  return list([
    t("itemRow.kind", { kind: item.kind }),
    ...(requirement === null ? [] : [requirementText(requirement)]),
    t("statusChip.state", { state: item.state }),
    ...(item.due === undefined ? [] : [due.label]),
  ]);
}
