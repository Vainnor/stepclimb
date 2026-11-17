// SPDX-License-Identifier: AGPL-3.0-or-later
// ProgressRail: a module's items drawn as a route, a line with one node per item, where the
// segment after a complete item fills with the brand color (docs/specs/m1/16-learning-ui-components.md
// B8; plan, UI tab -> Signature components). It is an ordered list of links: Up and Down move
// between them, Home and End jump to the ends. A locked item stays a focusable link (the item view
// says why it is locked) and carries what unlocks it as its description, shown on hover and focus.
//
// It holds the one celebratory motion in the product: a segment fills over the slow motion token
// (320 ms) when its item completes while the rail is shown, and when the course completes the
// whole rail fills once, segment after segment, in the same time. Both are CSS, so base.css's
// reduced-motion rule makes them instant.
import { CircleIcon } from "@phosphor-icons/react";
import { type CSSProperties, type KeyboardEvent, type ReactNode, useId, useState } from "react";
import { tv } from "../lib/tv";
import { Link } from "../primitives/link";
import { Tooltip, TooltipTrigger } from "../primitives/tooltip";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { DueBadge } from "./due-badge";
import { type LockReason, useLockReasonText } from "./lock-reason";
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
      "flex min-h-(--sc-control-h) gap-3 rounded-sm ps-2 pe-3 text-fg-default no-underline outline-none",
      "pointer-coarse:min-h-(--sc-control-touch)",
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
      // The course-completion fill: each segment in turn, all of them over the slow token.
      "group-data-[celebrate]/rail:animate-[sc-rail-fill_calc(var(--sc-motion-slow)/var(--rail-segments))_linear_both]",
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
    // The current item is ringed in the brand color unless the overdue ring already marks it.
    {
      current: true,
      state: ["locked", "available", "progress", "complete", "review"],
      class: { node: "border-2 border-brand" },
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
   * Whether the course is complete for the learner. When it turns true while the rail is shown,
   * the whole rail fills once; a rail first drawn complete does not.
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
  timeZone: string;
  now: Date;
}

function RailStep({ item, index, last, current, timeZone, now }: RailStepProps) {
  const { t } = useStepclimbStrings();
  const reasonText = useLockReasonText(timeZone);
  const reasonsId = useId();
  const styles = progressRail({ state: item.state, current, filled: item.state === "complete" });
  const reasons = item.state === "locked" ? item.lockReasons.map(reasonText) : [];
  const link = (
    <Link
      data-slot="progress-rail-link"
      data-state={item.state}
      href={item.href}
      aria-current={current ? "page" : undefined}
      aria-describedby={reasons.length > 0 ? reasonsId : undefined}
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
          <VisuallyHidden data-slot="progress-rail-state">
            {`, ${t("statusChip.state", { state: item.state })}`}
          </VisuallyHidden>
        </span>
        {item.state === "overdue" ? (
          <DueBadge due={item.due} now={now} timeZone={timeZone} />
        ) : null}
      </span>
    </Link>
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
            {reasons.join(" ")}
          </VisuallyHidden>
        </>
      )}
    </li>
  );
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
