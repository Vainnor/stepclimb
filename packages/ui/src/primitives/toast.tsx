// SPDX-License-Identifier: AGPL-3.0-or-later
// Toast: a short notice of what just happened, shown in a region at the bottom inline end of wide
// screens and at the top of phones (docs/specs/m0/13-base-ui-components.md M0 inventory, Q7). The
// app makes one queue with `createToastQueue()`, renders one `ToastRegion` for it, and adds toasts
// to the queue. Built on React Aria's toast parts, which are `UNSTABLE_` exports in 1.22.1: this
// file is the only one that imports them (ADR 0024).
//
// A new toast is announced through a live region and never takes focus. The region is a landmark
// (F6 reaches it); hovering it or focusing inside it pauses every timer. An error toast has no
// timer: it stays until it is dismissed. A toast may carry one action (the app's "A new version
// is available" with "Reload", docs/specs/m1/15-installable-web-app-and-offline-reading.md B5);
// such a toast has no timer either, so the action is still there when keyboard or screen reader
// users reach it. It does not move: no motion to reduce.
import { CheckCircleIcon, InfoIcon, WarningCircleIcon, XIcon } from "@phosphor-icons/react";
import {
  Text as AriaText,
  UNSTABLE_Toast as AriaToast,
  UNSTABLE_ToastContent as AriaToastContent,
  UNSTABLE_ToastList as AriaToastList,
  UNSTABLE_ToastQueue as AriaToastQueue,
  UNSTABLE_ToastRegion as AriaToastRegion,
  type QueuedToast,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { useAppearanceAttributes } from "../theme/theme-provider";
import { Button } from "./button";
import { IconButton } from "./icon-button";

/** How long a toast that is not an error stays, unless the caller asks for longer. */
export const TOAST_TIMEOUT_MS = 6000;

/**
 * The shortest time a toast that is not an error stays. A shorter `timeout` is raised to it:
 * React Aria's guidance for reading a toast before it goes, and the timer pauses on hover and
 * focus besides (WCAG 2.2.1).
 */
export const TOAST_MIN_TIMEOUT_MS = 5000;

export const toast = tv({
  slots: {
    // Phones: across the top, inside the top and side safe areas, clear of the BottomTabs and the
    // on-screen keyboard. 768 px and wider: the bottom inline end.
    region: [
      "fixed inset-x-3 top-[max(0.75rem,env(safe-area-inset-top))] z-60 outline-none",
      "ps-[env(safe-area-inset-left)] pe-[env(safe-area-inset-right)]",
      "rtl:ps-[env(safe-area-inset-right)] rtl:pe-[env(safe-area-inset-left)]",
      "md:inset-x-auto md:end-6 md:top-auto md:bottom-[max(1.5rem,env(safe-area-inset-bottom))] md:w-96",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
    ],
    list: "flex flex-col gap-2",
    root: [
      "flex items-start gap-3 rounded-md border bg-raised p-3 ps-4 font-ui shadow-float outline-none",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
    ],
    icon: "mt-0.5 shrink-0",
    content: "flex min-w-0 flex-1 flex-col gap-0.5 py-0.5",
    action: "mt-2 self-start",
    title: "text-sm font-medium text-fg-strong",
    description: "text-sm text-fg-sub",
    close: "-me-1 -mt-1 shrink-0",
  },
  variants: {
    tone: {
      neutral: { root: "border-line-strong", icon: "text-info-fg" },
      success: { root: "border-line-strong", icon: "text-success-fg" },
      error: { root: "border-danger", icon: "text-danger-fg" },
    },
  },
  defaultVariants: { tone: "neutral" },
});

export type ToastTone = "neutral" | "success" | "error";

/** What a toast says. Text comes from `useStepclimbStrings()`. */
export interface ToastMessage {
  /** One short sentence: what happened. */
  title: string;
  /** What to do next, when there is something to do. */
  description?: string;
  /** Neutral by default. An error stays until it is dismissed; it never has a timer. */
  tone?: ToastTone;
  /**
   * One thing the person can do about it, such as reloading. A toast with an action stays until
   * it is dismissed or the action is taken; it never has a timer.
   */
  action?: ToastAction;
}

/** A toast's one action: a button under its text. Pressing it runs `onAction` and closes it. */
export interface ToastAction {
  /** A verb, from `useStepclimbStrings()`. */
  label: string;
  onAction: () => void;
}

export interface ToastOptions {
  /**
   * How long a toast that is not an error stays, in milliseconds: TOAST_TIMEOUT_MS by default,
   * at least TOAST_MIN_TIMEOUT_MS. An error toast and a toast with an action ignore it.
   */
  timeout?: number;
  /** Called once the toast has closed, by its timer or by a person. */
  onClose?: () => void;
}

/** The queue an app adds toasts to; give it to one `ToastRegion`. */
export interface ToastQueue {
  /** Shows a toast, or queues it behind the visible ones; returns its key. */
  add(message: ToastMessage, options?: ToastOptions): string;
  /** Closes the toast with this key. */
  close(key: string): void;
}

const ariaQueues = new WeakMap<ToastQueue, AriaToastQueue<ToastMessage>>();

/**
 * The timeout React Aria gets: none for an error or a toast with an action, at least the minimum
 * for anything else.
 */
export function toastTimeout(message: ToastMessage, timeout?: number): number | undefined {
  if (message.tone === "error" || message.action !== undefined) return undefined;
  return Math.max(timeout ?? TOAST_TIMEOUT_MS, TOAST_MIN_TIMEOUT_MS);
}

/**
 * A queue showing at most `maxVisibleToasts` toasts at once (3 by default), the newest first; the
 * rest wait until one closes.
 */
export function createToastQueue({ maxVisibleToasts = 3 } = {}): ToastQueue {
  const queue = new AriaToastQueue<ToastMessage>({ maxVisibleToasts });
  const api: ToastQueue = {
    add: (message, options = {}) =>
      queue.add(message, {
        timeout: toastTimeout(message, options.timeout),
        onClose: options.onClose,
      }),
    close: (key) => queue.close(key),
  };
  ariaQueues.set(api, queue);
  return api;
}

const icons = { neutral: InfoIcon, success: CheckCircleIcon, error: WarningCircleIcon } as const;

function ToastItem({
  queued,
  queue,
}: {
  queued: QueuedToast<ToastMessage>;
  queue: AriaToastQueue<ToastMessage>;
}) {
  const { t } = useStepclimbStrings();
  const { title, description, tone = "neutral", action } = queued.content;
  const styles = toast({ tone });
  return (
    <AriaToast data-slot="toast" data-tone={tone} toast={queued} className={styles.root()}>
      <Icon icon={icons[tone]} size="md" className={styles.icon()} />
      <AriaToastContent data-slot="toast-content" className={styles.content()}>
        <AriaText data-slot="toast-title" slot="title" className={styles.title()}>
          {title}
        </AriaText>
        {description === undefined ? null : (
          <AriaText
            data-slot="toast-description"
            slot="description"
            className={styles.description()}
          >
            {description}
          </AriaText>
        )}
        {action === undefined ? null : (
          // `slot={null}`: the toast's button context names only its close slot.
          <Button
            slot={null}
            data-slot="toast-action"
            size="sm"
            className={styles.action()}
            onPress={() => {
              queue.close(queued.key);
              action.onAction();
            }}
          >
            {action.label}
          </Button>
        )}
      </AriaToastContent>
      <IconButton
        data-slot="toast-close"
        slot="close"
        icon={XIcon}
        label={t("toast.close")}
        size="sm"
        className={styles.close()}
      />
    </AriaToast>
  );
}

export interface ToastRegionProps {
  /** The queue from `createToastQueue()`. */
  queue: ToastQueue;
  /** Layout only. */
  className?: string;
}

/**
 * Renders the queue's visible toasts, portaled to the end of `body` in the surrounding
 * ThemeProvider's appearance; nothing while the queue is empty. Render one per queue, once, near
 * the root of the app.
 */
export function ToastRegion({ queue, className }: ToastRegionProps) {
  const { t } = useStepclimbStrings();
  const appearance = useAppearanceAttributes();
  const ariaQueue = ariaQueues.get(queue);
  if (!ariaQueue) throw new Error("ToastRegion needs a queue made by createToastQueue()");
  const styles = toast();
  return (
    <AriaToastRegion
      data-slot="toast-region"
      queue={ariaQueue}
      aria-label={t("toast.region")}
      {...appearance}
      className={styles.region({ className })}
    >
      <AriaToastList<ToastMessage> data-slot="toast-list" className={styles.list()}>
        {({ toast: queued }) => <ToastItem queued={queued} queue={ariaQueue} />}
      </AriaToastList>
    </AriaToastRegion>
  );
}
