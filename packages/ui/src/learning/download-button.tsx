// SPDX-License-Identifier: AGPL-3.0-or-later
// DownloadButton: a course's offline download, in the state the caller reports, with the actions
// that state allows (docs/specs/m1/16-learning-ui-components.md B12;
// docs/specs/m1/15-installable-web-app-and-offline-reading.md B8, B11, B13). Not downloaded: a
// Download button, the package's size and the note that downloads are not encrypted.
// Downloading: a progress bar and Cancel. Downloaded: when and how large, and Remove. Update
// available: the same with Update. The component draws; the caller fetches, stores, measures
// and refuses (a package over the size limit or the free quota, spec 15 B8) and passes the
// refusal as `problem`, which a live region announces. A package too large to store is not
// offered for download again. `isDisabled` turns off the actions that need the server (Download,
// Update) while the app is offline; Remove and Cancel need no connection and stay.
import { ArrowsClockwiseIcon, DownloadSimpleIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { useId, useMemo } from "react";
import { useLocale } from "react-aria-components";
import { tv } from "../lib/tv";
import { sizeValues } from "../patterns/file-drop";
import { Button } from "../primitives/button";
import { ProgressBar } from "../primitives/progress-bar";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

/** Where a course's offline download stands. */
export type DownloadState =
  /** Not downloaded; `sizeBytes` is the package's `size_bytes` once the caller knows it. */
  | { kind: "none"; sizeBytes?: number }
  /** Being fetched: bytes stored so far of the package's total. */
  | { kind: "downloading"; receivedBytes: number; totalBytes: number }
  /** Stored, and its `version` matches the server's at the last check. */
  | { kind: "downloaded"; downloadedAt: Date; sizeBytes: number }
  /** Stored, and the server's package has a different `version` (spec 15 B11). */
  | { kind: "update"; downloadedAt: Date; sizeBytes: number };

/** Why the last download did not happen or did not finish (spec 15 B8). */
export type DownloadProblem =
  /** Over the largest download the app stores for one course. */
  | { kind: "tooLarge"; sizeBytes: number }
  /** Over the browser's reported free quota. */
  | { kind: "noRoom"; sizeBytes: number }
  /** Stopped partway; what it stored was removed. */
  | { kind: "failed" };

export const downloadButton = tv({
  slots: {
    root: "flex flex-col gap-3 font-ui text-sm",
    row: "flex flex-wrap items-center gap-x-4 gap-y-2",
    status: "flex min-w-0 flex-col gap-0.5",
    update: "font-medium text-fg-strong",
    detail: "text-fg-sub",
    progress: "min-w-0 flex-[1_1_12rem]",
    actions: "flex flex-wrap gap-2",
    note: "max-w-prose text-fg-sub",
    problem: "max-w-prose text-danger-fg empty:hidden",
  },
});

export interface DownloadButtonProps {
  state: DownloadState;
  /** The refusal or failure of the last attempt, shown until the caller clears it. */
  problem?: DownloadProblem;
  /** The user's IANA time zone (`users.tz`), in which the download date reads. */
  timeZone: string;
  /** Starts a download, from `none`. */
  onDownload: () => void;
  /** Stops a download in progress; the caller removes what it stored. */
  onCancel: () => void;
  /** Deletes the stored download. */
  onRemove: () => void;
  /** Fetches the server's newer package, from `update`. */
  onUpdate: () => void;
  /** Turns off Download and Update, which need the server: set it while the app is offline. */
  isDisabled?: boolean;
  /** Layout only. */
  className?: string;
}

export function DownloadButton({
  state,
  problem,
  timeZone,
  onDownload,
  onCancel,
  onRemove,
  onUpdate,
  isDisabled = false,
  className,
}: DownloadButtonProps) {
  const { t } = useStepclimbStrings();
  const styles = downloadButton();
  const problemText = useProblemText(problem);
  return (
    <div
      data-slot="download-button"
      data-state={state.kind}
      data-disabled={isDisabled || undefined}
      className={styles.root({ className })}
    >
      {state.kind === "none" ? (
        <NotDownloaded
          state={state}
          onDownload={onDownload}
          // A package too large to store is refused again however often it is tried.
          isDisabled={isDisabled || problem?.kind === "tooLarge"}
        />
      ) : null}
      {state.kind === "downloading" ? (
        <div data-slot="download-button-row" className={styles.row()}>
          <ProgressBar
            data-slot="download-button-progress"
            label={t("downloadButton.downloading")}
            value={state.receivedBytes}
            maxValue={state.totalBytes}
            className={styles.progress()}
          />
          <Button data-slot="download-button-cancel" intent="ghost" onPress={onCancel}>
            <Icon icon={XIcon} size="sm" />
            {t("downloadButton.cancel")}
          </Button>
        </div>
      ) : null}
      {state.kind === "downloaded" || state.kind === "update" ? (
        <Stored
          state={state}
          timeZone={timeZone}
          onRemove={onRemove}
          onUpdate={onUpdate}
          isDisabled={isDisabled}
        />
      ) : null}
      {/* In the page from the first render, so a problem is announced when it appears. */}
      <p data-slot="download-button-problem" role="alert" className={styles.problem()}>
        {problemText}
      </p>
    </div>
  );
}

function NotDownloaded({
  state,
  onDownload,
  isDisabled,
}: {
  state: { sizeBytes?: number };
  onDownload: () => void;
  isDisabled: boolean;
}) {
  const { t } = useStepclimbStrings();
  const noteId = useId();
  const styles = downloadButton();
  const size =
    state.sizeBytes === undefined ? null : t("fileDrop.size", sizeValues(state.sizeBytes));
  return (
    <>
      <div data-slot="download-button-actions" className={styles.actions()}>
        <Button
          data-slot="download-button-download"
          onPress={onDownload}
          isDisabled={isDisabled}
          aria-describedby={noteId}
        >
          <Icon icon={DownloadSimpleIcon} size="sm" />
          {t("downloadButton.download")}
        </Button>
      </div>
      <p id={noteId} data-slot="download-button-note" className={styles.note()}>
        {size === null ? t("downloadButton.note") : t("downloadButton.sizeNote", { size })}
      </p>
    </>
  );
}

interface StoredProps {
  state: { kind: "downloaded" | "update"; downloadedAt: Date; sizeBytes: number };
  timeZone: string;
  onRemove: () => void;
  onUpdate: () => void;
  isDisabled: boolean;
}

function Stored({ state, timeZone, onRemove, onUpdate, isDisabled }: StoredProps) {
  const { t } = useStepclimbStrings();
  const day = useDayFormat(timeZone);
  const styles = downloadButton();
  return (
    <div data-slot="download-button-row" className={styles.row()}>
      <p data-slot="download-button-status" className={styles.status()}>
        {state.kind === "update" ? (
          <span data-slot="download-button-update" className={styles.update()}>
            {t("downloadButton.updateAvailable")}
          </span>
        ) : null}
        <span data-slot="download-button-detail" className={styles.detail()}>
          {t("downloadButton.downloaded", {
            when: day(state.downloadedAt),
            size: t("fileDrop.size", sizeValues(state.sizeBytes)),
          })}
        </span>
      </p>
      <div data-slot="download-button-actions" className={styles.actions()}>
        {state.kind === "update" ? (
          <Button data-slot="download-button-refresh" onPress={onUpdate} isDisabled={isDisabled}>
            <Icon icon={ArrowsClockwiseIcon} size="sm" />
            {t("downloadButton.update")}
          </Button>
        ) : null}
        <Button data-slot="download-button-remove" intent="ghost" onPress={onRemove}>
          <Icon icon={TrashIcon} size="sm" />
          {t("downloadButton.remove")}
        </Button>
      </div>
    </div>
  );
}

/** The day `at` falls on in `timeZone`, as the locale writes a medium date. */
function useDayFormat(timeZone: string): (at: Date) => string {
  const { locale } = useLocale();
  return useMemo(() => {
    const format = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone });
    return (at: Date) => format.format(at);
  }, [locale, timeZone]);
}

function useProblemText(problem: DownloadProblem | undefined): string | null {
  const { t } = useStepclimbStrings();
  if (problem === undefined) return null;
  if (problem.kind === "failed") return t("downloadButton.problem.failed");
  const size = t("fileDrop.size", sizeValues(problem.sizeBytes));
  return problem.kind === "tooLarge"
    ? t("downloadButton.problem.tooLarge", { size })
    : t("downloadButton.problem.noRoom", { size });
}
