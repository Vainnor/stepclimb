// SPDX-License-Identifier: AGPL-3.0-or-later
// ProgressRail: a module's items drawn as a route, a line with one node per item, where the
// segment after a complete item fills with the brand color (docs/specs/m1/16-learning-ui-components.md
// B8; plan, UI tab -> Signature components). It is an ordered list of links: Up and Down move
// between them, Home and End jump to the ends. A locked item stays a focusable link (the item view
// says why it is locked) and carries what unlocks it as its description, shown on hover and focus.
//
// It holds the one celebratory motion in the product: a segment fills over the slow motion token
// (320 ms) when its item completes while the rail is shown, and when the course completes every
// segment fills, one after another, within the same time. Both are CSS on the motion tokens, so
// base.css's reduced-motion rule makes them instant.
import { CircleIcon } from "@phosphor-icons/react";
import { type CSSProperties, type KeyboardEvent, type ReactNode, useId, useState } from "react";
import { Link as AriaLink } from "react-aria-components";
import { tv } from "../lib/tv";
import { Tooltip, TooltipTrigger } from "../primitives/tooltip";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { DueBadge, useDueLabel } from "./due-badge";
import { type LockReason, useLockReasonList, useLockReasonText } from "./lock-reason";
import { type LearningState, learningStateIcons } from "./status-chip";

interface ProgressRailItemBase {
  id: string;
  /** The item's title, as the learner sees it. */
  title: string;
  /** The item view's path. */
  href: string;
}

/** One item on the rail, in the learning state the server reports for the learner. */
export type ProgressRailItem = ProgressRailItemBase &
  (
    | { state: "locked"; lockReasons: readonly [LockReason, ...LockReason[]] }
    | { state: "overdue"; due: Date }
    | { state: Exclude<LearningState, "locked" | "overdue"> }
  );

export const progressRail = tv({
  slots: {
    root: "group/rail flex flex-col font-ui text-sm text-fg-default",
    step: "relative",
    link: [
      "flex min-h-(--sc-control-h) cursor-pointer gap-3 rounded-sm ps-2 pe-3 text-fg-default outline-none",
      "pointer-coarse:min-h-(--sc-control-touch) transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-sunken data-[pressed]:bg-sunken",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-inset data-[focus-visible]:ring-ring",
    ],
    // The node's column. Its top padding sets the node's middle on the title's first line.
    track: "relative flex w-7 shrink-0 justify-center pt-1",
    node: "flex size-7 items-center justify-center rounded-full",
    nodeIcon: "",
    // The segment from this node to the next: a border, so it stays drawn in forced colors. It
    // reaches into the next step's top padding, to the next node.
    segment: "absolute inset-x-0 top-8 -bottom-1 mx-auto w-0.5 border-s-2 border-line-strong",
    fill: [
      // Over the segment's own border, which takes its whole width.
      "absolute inset-y-0 -start-0.5 w-0.5 origin-top border-s-2 border-brand",
      "transition-[scale] duration-slow ease-out-expo",
      // The course-completion fill: each segment in turn, all of them within the slow token.
      "group-data-[celebrate]/rail:animate-[sc-rail-fill_calc(var(--sc-motion-slow)/var(--rail-segments))_var(--sc-ease-out-expo)_both]",
      "group-data-[celebrate]/rail:[animation-delay:calc(var(--sc-motion-slow)*var(--rail-step)/var(--rail-segments))]",
    ],
    content: "flex min-w-0 flex-1 flex-col items-start gap-1 py-2",
    title: "break-words",
    reasons: "flex flex-col gap-1",
    empty: "px-3 py-2 text-fg-sub",
  },
  variants: {
    state: {
      locked: { nodeIcon: "text-state-locked", title: "text-fg-soft" },
      available: { nodeIcon: "text-state-available" },
      progress: { nodeIcon: "text-state-progress" },
      complete: { nodeIcon: "text-state-complete" },
      overdue: { node: "border-2 border-state-overdue", nodeIcon: "text-state-overdue" },
      review: { nodeIcon: "text-state-review" },
    },
    current: { true: { title: "font-control text-fg-strong" } },
    filled: { true: { fill: "scale-y-100" }, false: { fill: "scale-y-0" } },
  },
  compoundVariants: [
    // The current item is ringed in the brand color. An overdue item keeps its overdue ring, and
    // the brand ring goes around it.
    {
      current: true,
      state: ["locked", "available", "progress", "complete", "review"],
      class: { node: "border-2 border-brand" },
    },
    {
      current: true,
      state: "overdue",
      class: { node: "outline-2 outline-offset-1 outline-brand" },
    },
    { current: true, state: "available", class: { nodeIcon: "size-2.5 text-brand" } },
  ],
});

export interface ProgressRailProps {
  /** The module's items, in module order. */
  items: readonly ProgressRailItem[];
  /** The item being shown, marked as the current page. */
  currentId?: string;
  /**
   * Whether the course is complete for the learner: every segment is then filled. When it turns
   * true while the rail is shown, the whole rail fills once; a rail first drawn complete does not.
   */
  courseComplete?: boolean;
  /** The user's IANA time zone (`users.tz`), for due labels and opening dates. */
  timeZone: string;
  /** The current instant, from the app's clock, for due labels. */
  now: Date;
  /** What the rail shows when the module has no items. */
  emptyState?: ReactNode;
  "aria-label"?: string;
  "aria-labelledby"?: string;
  /** Layout only. */
  className?: string;
}

/** Name it with `aria-label` or `aria-labelledby`, usually the module's title. */
export function ProgressRail({
  items,
  currentId,
  courseComplete = false,
  timeZone,
  now,
  emptyState,
  className,
  ...aria
}: ProgressRailProps) {
  const celebrate = useCompletionMoment(courseComplete);
  const styles = progressRail();
  if (items.length === 0) {
    return emptyState === undefined ? null : (
      <div data-slot="progress-rail-empty" className={styles.empty({ className })}>
        {emptyState}
      </div>
    );
  }
  return (
    <ol
      data-slot="progress-rail"
      data-celebrate={celebrate || undefined}
      {...aria}
      onKeyDown={moveFocus}
      style={{ "--rail-segments": Math.max(items.length - 1, 1) } as CSSProperties}
      className={styles.root({ className })}
    >
      {items.map((item, index) => (
        <RailStep
          key={item.id}
          item={item}
          index={index}
          last={index === items.length - 1}
          current={item.id === currentId}
          courseComplete={courseComplete}
          timeZone={timeZone}
          now={now}
        />
      ))}
    </ol>
  );
}

/**
 * True from the render in which `courseComplete` turns true, for as long as the rail stays
 * mounted, so the course-completion fill runs once. A rail first drawn complete stays false.
 */
function useCompletionMoment(courseComplete: boolean): boolean {
  const [previous, setPrevious] = useState(courseComplete);
  const [celebrate, setCelebrate] = useState(false);
  if (courseComplete !== previous) {
    setPrevious(courseComplete);
    if (courseComplete) setCelebrate(true);
  }
  return celebrate;
}

const MOVES: Record<string, (index: number, count: number) => number> = {
  ArrowDown: (i, n) => Math.min(i + 1, n - 1),
  ArrowUp: (i) => Math.max(i - 1, 0),
  Home: () => 0,
  End: (_, n) => n - 1,
};

/** Up, Down, Home and End move focus between the rail's links. */
function moveFocus(event: KeyboardEvent<HTMLOListElement>) {
  const move = MOVES[event.key];
  if (!move || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const links = [
    ...event.currentTarget.querySelectorAll<HTMLElement>('[data-slot="progress-rail-link"]'),
  ];
  const index =
    document.activeElement instanceof HTMLElement ? links.indexOf(document.activeElement) : -1;
  if (index === -1) return;
  event.preventDefault();
  links[move(index, links.length)]?.focus();
}

interface RailStepProps {
  item: ProgressRailItem;
  index: number;
  last: boolean;
  current: boolean;
  courseComplete: boolean;
  timeZone: string;
  now: Date;
}

function RailStep({ item, index, last, current, courseComplete, timeZone, now }: RailStepProps) {
  const reasonText = useLockReasonText(timeZone);
  const reasonList = useLockReasonList();
  const reasonsId = useId();
  const styles = progressRail();
  const reasons = item.state === "locked" ? item.lockReasons.map(reasonText) : [];
  const link = (
    <RailLink
      item={item}
      last={last}
      current={current}
      filled={courseComplete || item.state === "complete"}
      describedBy={reasons.length > 0 ? reasonsId : undefined}
      timeZone={timeZone}
      now={now}
    />
  );
  return (
    <li
      data-slot="progress-rail-step"
      data-state={item.state}
      style={{ "--rail-step": index } as CSSProperties}
      className={styles.step()}
    >
      {reasons.length === 0 ? (
        link
      ) : (
        <>
          <TooltipTrigger>
            {link}
            <Tooltip data-slot="progress-rail-tooltip" placement="end">
              <span className={styles.reasons()}>
                {reasons.map((reason) => (
                  <span key={reason}>{reason}</span>
                ))}
              </span>
            </Tooltip>
          </TooltipTrigger>
          <VisuallyHidden id={reasonsId} data-slot="progress-rail-reasons">
            {reasonList(reasons)}
          </VisuallyHidden>
        </>
      )}
    </li>
  );
}

interface RailLinkProps {
  item: ProgressRailItem;
  last: boolean;
  current: boolean;
  filled: boolean;
  describedBy: string | undefined;
  timeZone: string;
  now: Date;
}

/** The node, the segment after it and the title, as one link named by the title and state. */
function RailLink({ item, last, current, filled, describedBy, timeZone, now }: RailLinkProps) {
  const styles = progressRail({ state: item.state, current, filled });
  const name = useRailItemName(item, timeZone, now);
  return (
    <AriaLink
      data-slot="progress-rail-link"
      data-state={item.state}
      href={item.href}
      aria-label={name}
      aria-current={current ? "page" : undefined}
      aria-describedby={describedBy}
      className={styles.link()}
    >
      <span data-slot="progress-rail-track" className={styles.track()}>
        <span data-slot="progress-rail-node" className={styles.node()}>
          <NodeIcon state={item.state} current={current} className={styles.nodeIcon()} />
        </span>
        {last ? null : (
          <span data-slot="progress-rail-segment" aria-hidden="true" className={styles.segment()}>
            <span data-slot="progress-rail-fill" className={styles.fill()} />
          </span>
        )}
      </span>
      <span data-slot="progress-rail-content" className={styles.content()}>
        <span data-slot="progress-rail-title" className={styles.title()}>
          {item.title}
        </span>
        {item.state === "overdue" ? (
          <>
            {/* Separates the title's text from the badge's where text is read in one run (axe's
                label-in-name check); a flex container draws no white space between items. */}{" "}
            <DueBadge due={item.due} now={now} timeZone={timeZone} />
          </>
        ) : null}
      </span>
    </AriaLink>
  );
}

/**
 * The link's name: the title, then the state. An overdue item's state is its due label, which the
 * link shows, so the name holds all the text the link draws (WCAG 2.5.3).
 */
function useRailItemName(item: ProgressRailItem, timeZone: string, now: Date): string {
  const { t } = useStepclimbStrings();
  // Called for every item, so the hooks run in the same order; only an overdue item uses it.
  const due = useDueLabel(item.state === "overdue" ? item.due : now, now, timeZone);
  if (item.state === "overdue") {
    return t("progressRail.overdueItem", { title: item.title, due: due.label });
  }
  const state = t("statusChip.state", { state: item.state });
  return t("progressRail.item", { title: item.title, state });
}

/** The state's icon (spec 16 B3); the current item, while available, is a dot in its ring. */
function NodeIcon({
  state,
  current,
  className,
}: {
  state: LearningState;
  current: boolean;
  className: string;
}) {
  if (current && state === "available") {
    return <Icon icon={CircleIcon} state="complete" className={className} />;
  }
  const { icon, weight } = learningStateIcons[state];
  return <Icon icon={icon} state={weight} size="lg" className={className} />;
}
