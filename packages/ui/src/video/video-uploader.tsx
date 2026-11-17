// SPDX-License-Identifier: AGPL-3.0-or-later
// VideoUploader: uploads a video to a course through tus in 16 MiB chunks, with progress, pause,
// resume and cancel; resumes after a reload when the same file is chosen again; then shows the
// video processing until it is ready (docs/specs/m1/08-uploaded-video.md B22). The protocol is
// `tus.ts`, the state `use-video-upload.ts`. Each request goes through the caller's `send` and the
// video is read through its `getVideo`, so the component knows no API client. The server decides
// every refusal and every outcome; the type check of FileDrop only saves a wasted upload.
//
// A polite status region says what the upload is doing and its progress in steps of 10 %, so a
// screen reader is told without being flooded; a refusal goes to the card's alert region.
import {
  CheckCircleIcon,
  HourglassIcon,
  PauseIcon,
  PlayIcon,
  WarningCircleIcon,
  XIcon,
} from "@phosphor-icons/react";
import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { tv } from "../lib/tv";
import { FileDrop, sizeValues } from "../patterns/file-drop";
import { Button } from "../primitives/button";
import { Code } from "../primitives/code";
import { Heading } from "../primitives/heading";
import { ProgressBar } from "../primitives/progress-bar";
import { Text } from "../primitives/text";
import { VisuallyHidden } from "../primitives/visually-hidden";
import { type Strings, useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import type { ResumeEntry } from "./resume-store";
import type { Send } from "./tus";
import {
  type GetVideo,
  STOPPED,
  type UploadPhase,
  type UploadState,
  useVideoUpload,
} from "./use-video-upload";
import { isResumable, problemText, type UploadProblem } from "./video-problems";

export const videoUploader = tv({
  slots: {
    root: "flex min-w-0 flex-col gap-4",
    card: "flex min-w-0 flex-col gap-3 rounded-md border border-line bg-raised p-4 font-ui",
    title: "flex min-w-0 items-start gap-2",
    name: "min-w-0 wrap-anywhere outline-none",
    stateIcon: "mt-0.5 size-[1.1em] shrink-0 text-fg-sub",
    actions: "flex flex-wrap gap-2",
    alert: "flex flex-col gap-1 text-sm text-danger-fg empty:hidden",
    alertLine: "flex items-start gap-1.5",
    alertIcon: "mt-0.5 size-[1.1em] shrink-0",
    pending: "flex min-w-0 flex-col gap-2",
    pendingList: "flex flex-col gap-2",
    pendingItem:
      "flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-md border border-line bg-raised px-4 py-2",
  },
});

const styles = videoUploader();

/** What the file picker offers: video types, and the extensions some systems give no type. */
export const VIDEO_FILE_TYPES = ["video/*", ".mp4", ".m4v", ".mov", ".webm", ".mkv"] as const;

/** The 10 % step `sent` of `size` has reached. */
function step(sent: number, size: number): number {
  return size === 0 ? 10 : Math.floor((sent / size) * 10);
}

function bytes(t: Strings["t"], count: number): string {
  return t("fileDrop.size", sizeValues(count));
}

/** What the upload is doing, in one sentence. */
export function phaseText(state: UploadState, t: Strings["t"]): string {
  return t("videoUploader.phase", { phase: state.phase, name: state.name });
}

/** The polite announcement for `state`: its phase, or its progress in 10 % steps. */
function announcement(state: UploadState | undefined, t: Strings["t"]): string {
  if (state === undefined) return "";
  if (state.phase !== "uploading") return phaseText(state, t);
  const share = Math.min(1, step(state.sent, state.size) / 10);
  return t("videoUploader.progress", { name: state.name, share });
}

export interface VideoUploadCardProps {
  state: UploadState;
  onPause?: () => void;
  onResume?: () => void;
  onCancel?: () => void;
}

const SHOWS_PROGRESS: ReadonlySet<UploadPhase> = new Set([
  "starting",
  "uploading",
  "paused",
  "interrupted",
  "cancelling",
]);

function StateIcon({ phase }: { phase: UploadPhase }) {
  if (phase === "ready") return <Icon icon={CheckCircleIcon} className={styles.stateIcon()} />;
  if (phase === "processing") return <Icon icon={HourglassIcon} className={styles.stateIcon()} />;
  return null;
}

function Alert({ problem, name }: { problem: UploadProblem | undefined; name: string }) {
  const { t } = useStepclimbStrings();
  const text = problem === undefined ? undefined : problemText(problem, name, t);
  return (
    // In the page from the first render, so a refusal is announced when it appears.
    <div data-slot="video-upload-alert" role="alert" className={styles.alert()}>
      {text === undefined ? null : (
        <p className={styles.alertLine()}>
          <Icon icon={WarningCircleIcon} className={styles.alertIcon()} />
          <span>
            {text.message}
            {text.code === undefined ? null : (
              <>
                {" "}
                <Code>{text.code}</Code>
              </>
            )}
          </span>
        </p>
      )}
    </div>
  );
}

/**
 * One upload: its name and phase, its progress while bytes are going, and Pause or Resume (one
 * button, so focus stays on it) and Cancel while it can be stopped. When those buttons go, focus
 * that was on them moves to the upload's name.
 */
export function VideoUploadCard({ state, onPause, onResume, onCancel }: VideoUploadCardProps) {
  const { t } = useStepclimbStrings();
  const titleRef = useRef<HTMLSpanElement>(null);
  const { phase, name, size, sent, problem } = state;
  const stopped = STOPPED.has(phase);
  const controls = phase === "uploading" || phase === "cancelling" || stopped;
  const hadFocus = useRef(false);
  useEffect(() => {
    if (!controls && hadFocus.current) titleRef.current?.focus();
    hadFocus.current = false;
  }, [controls]);
  return (
    <section data-slot="video-upload" data-phase={phase} className={styles.card()}>
      <Heading level={3} size="base" className={styles.title()}>
        <StateIcon phase={phase} />
        <span ref={titleRef} tabIndex={-1} className={styles.name()}>
          {phaseText(state, t)}
        </span>
      </Heading>
      {state.origin === "new" || phase !== "uploading" ? null : (
        <Text elementType="p" size="sm" tone="sub">
          {state.origin === "resumed"
            ? t("videoUploader.resumed", { name })
            : t("videoUploader.restarted", { name })}
        </Text>
      )}
      {SHOWS_PROGRESS.has(phase) ? (
        <ProgressBar
          data-slot="video-upload-progress"
          aria-label={t("videoUploader.progressLabel", { name })}
          value={size === 0 ? 100 : (sent / size) * 100}
          valueLabel={t("videoUploader.bytes", {
            share: size === 0 ? 1 : sent / size,
            sent: bytes(t, sent),
            size: bytes(t, size),
          })}
        />
      ) : null}
      {phase === "processing" ? (
        <Text elementType="p" size="sm" tone="sub">
          {state.pollFailing ? t("videoUploader.pollFailing") : t("videoUploader.processingNote")}
        </Text>
      ) : null}
      {phase === "interrupted" && problem !== undefined && isResumable(problem) ? (
        <Text elementType="p" size="sm" tone="sub">
          {t("videoUploader.resumeNote")}
        </Text>
      ) : null}
      <Alert problem={problem} name={name} />
      {controls ? (
        <CardActions
          phase={phase}
          onPause={onPause}
          onResume={onResume}
          onCancel={onCancel}
          onUnmountWithFocus={() => {
            hadFocus.current = true;
          }}
        />
      ) : null}
    </section>
  );
}

interface CardActionsProps extends Omit<VideoUploadCardProps, "state"> {
  phase: UploadPhase;
  /** Called as the buttons leave the page while one of them has focus. */
  onUnmountWithFocus: () => void;
}

function CardActions({ phase, onPause, onResume, onCancel, onUnmountWithFocus }: CardActionsProps) {
  const { t } = useStepclimbStrings();
  const ref = useRef<HTMLDivElement>(null);
  const stopped = STOPPED.has(phase);
  const lostFocus = useRef(onUnmountWithFocus);
  lostFocus.current = onUnmountWithFocus;
  // A layout cleanup runs before React removes the buttons, so the focused one is still there.
  useLayoutEffect(() => {
    const actions = ref.current;
    return () => {
      if (actions?.contains(document.activeElement)) lostFocus.current();
    };
  }, []);
  return (
    <div ref={ref} data-slot="video-upload-actions" className={styles.actions()}>
      <Button
        data-slot="video-upload-toggle"
        isDisabled={phase === "cancelling"}
        onPress={stopped ? onResume : onPause}
      >
        <Icon icon={stopped ? PlayIcon : PauseIcon} />
        {stopped ? t("videoUploader.resume") : t("videoUploader.pause")}
      </Button>
      <Button
        data-slot="video-upload-cancel"
        intent="danger"
        isPending={phase === "cancelling"}
        onPress={onCancel}
      >
        <Icon icon={XIcon} />
        {t("videoUploader.cancel")}
      </Button>
    </div>
  );
}

function PendingItem({
  entry,
  onDiscard,
}: {
  entry: ResumeEntry;
  onDiscard: (entry: ResumeEntry) => Promise<UploadProblem | undefined>;
}) {
  const { t } = useStepclimbStrings();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<UploadProblem>();
  const share = entry.size === 0 ? 0 : entry.offset / entry.size;
  return (
    <li data-slot="video-upload-pending-item" className={styles.pendingItem()}>
      <Text size="sm" className="min-w-0 wrap-anywhere">
        {t("videoUploader.pending.item", { name: entry.name, share })}
      </Text>
      <Button
        size="sm"
        aria-label={t("videoUploader.pending.discardLabel", { name: entry.name })}
        isPending={busy}
        onPress={async () => {
          setBusy(true);
          const refused = await onDiscard(entry);
          setBusy(false);
          setProblem(refused);
        }}
      >
        {t("videoUploader.pending.discard")}
      </Button>
      <div className="basis-full empty:hidden">
        <Alert problem={problem} name={entry.name} />
      </div>
    </li>
  );
}

export interface VideoUploaderProps {
  /** The creation address, such as `/api/v1/courses/<id>/video-uploads`. */
  endpoint: string;
  /** Whose unfinished uploads local storage keeps together: the course's id. */
  storageKey: string;
  /** Sends one request; the web app sends through `XMLHttpRequest` for progress. */
  send: Send;
  /** Reads a video's status; `null` when it no longer exists. */
  getVideo: GetVideo;
  /** Called when a video is created, completes, settles or is deleted, to refresh a list. */
  onChange?: () => void;
  /** Local storage; `globalThis.localStorage` by default. */
  storage?: Storage;
  /** Every chunk but the last, at least 1 MiB; 16 MiB by default (B22). */
  chunkBytes?: number;
  isDisabled?: boolean;
  /** Layout only. */
  className?: string;
}

/** Chooses a video and uploads it; see the file's header. */
export function VideoUploader({ isDisabled, className, ...options }: VideoUploaderProps) {
  const { t } = useStepclimbStrings();
  const upload = useVideoUpload(options);
  const [busy, setBusy] = useState<string>();
  const { state, pending } = upload;
  const choose = ([file]: File[]) => {
    if (file === undefined) return;
    setBusy(upload.start(file) ? undefined : t("videoUploader.busy", { name: state?.name ?? "" }));
  };
  return (
    <div data-slot="video-uploader" className={styles.root({ className })}>
      <FileDrop
        aria-label={t("videoUploader.dropTarget")}
        acceptedFileTypes={VIDEO_FILE_TYPES}
        description={t("videoUploader.types")}
        onSelect={choose}
        isDisabled={isDisabled}
        errorMessage={busy}
      />
      {pending.length === 0 ? null : <PendingList entries={pending} onDiscard={upload.discard} />}
      {state === undefined ? null : (
        <VideoUploadCard
          state={state}
          onPause={upload.pause}
          onResume={upload.resume}
          onCancel={upload.cancel}
        />
      )}
      <VisuallyHidden data-slot="video-uploader-status" elementType="p" role="status">
        {announcement(state, t)}
      </VisuallyHidden>
    </div>
  );
}

function PendingList({
  entries,
  onDiscard,
}: {
  entries: ResumeEntry[];
  onDiscard: (entry: ResumeEntry) => Promise<UploadProblem | undefined>;
}): ReactNode {
  const { t } = useStepclimbStrings();
  return (
    <section data-slot="video-upload-pending" className={styles.pending()}>
      <Heading level={3} size="base">
        {t("videoUploader.pending.heading")}
      </Heading>
      <Text elementType="p" size="sm" tone="sub">
        {t("videoUploader.pending.description")}
      </Text>
      <ul className={styles.pendingList()}>
        {entries.map((entry) => (
          <PendingItem key={entry.url} entry={entry} onDiscard={onDiscard} />
        ))}
      </ul>
    </section>
  );
}
