// SPDX-License-Identifier: AGPL-3.0-or-later
// FileDrop: a drop zone with a "Choose file" button, for course file uploads and image insertion
// (docs/specs/m1/16-learning-ui-components.md B6). Built on React Aria's DropZone and FileTrigger:
// a file can be dropped, chosen with the button or pasted into the zone's hidden drop button from
// the keyboard. A drag that carries no file is refused while it is over the zone. It refuses a
// file of a type or size the caller does not accept, saying which file and why, before handing
// the rest to `onSelect`. Those checks save a person a wasted upload and are not a control: the
// server (docs/specs/m1/06-files.md B4, B6) refuses only a file above the instance's
// `max_file_bytes` and one that does not fit the storage limit, and records the type it sniffs
// without refusing any. It shows an upload's progress, which the caller reports.
import { UploadSimpleIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { type ReactNode, useRef, useState } from "react";
import {
  DropZone,
  type DropZoneProps,
  FileTrigger,
  isFileDropItem,
  Text,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { Button } from "../primitives/button";
import { ProgressBar } from "../primitives/progress-bar";
import { type MessageValues, useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const fileDrop = tv({
  slots: {
    root: [
      "group flex flex-col items-center gap-3 rounded-md border border-dashed border-line-strong bg-raised",
      "px-6 py-8 text-center font-ui text-sm text-fg-default outline-none",
      "transition-colors duration-fast ease-out-expo",
      // React Aria keeps the zone hovered while a file is dragged over it (no pointer event
      // leaves it during a drag), and Tailwind orders hover after the drop target, so hover leaves
      // a drop target's border alone instead of relying on that order.
      "data-[hovered]:not-data-[drop-target]:border-fg-sub",
      "data-[drop-target]:border-solid data-[drop-target]:border-brand data-[drop-target]:bg-brand-soft",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "data-[disabled]:cursor-not-allowed data-[disabled]:border-line data-[disabled]:text-fg-disabled",
    ],
    // Disabled shows in the icon and the button; the text keeps its contrast, as it is not a
    // control.
    icon: "text-fg-sub group-data-[disabled]:text-fg-disabled",
    // On a touch screen there is nothing to drag from: the prompt stays only for screen readers,
    // as it names the zone's drop button.
    prompt: "font-medium text-fg-strong pointer-coarse:sr-only",
    limits: "text-caption text-fg-sub",
    progress: "w-full max-w-sm text-start",
    errors:
      "flex w-full max-w-sm flex-col gap-1.5 text-start text-caption text-danger-fg empty:hidden",
    error: "flex items-start gap-1.5",
    errorIcon: "mt-px size-[1.1em]",
  },
});

const styles = fileDrop();

/** A refusal shown in the zone; the id keys it among others with the same text. */
type Problem = { id: number; text: string };

/** Why a file was refused. */
export type FileRejection = { file: File; reason: "type" | "size" };

/** The upload in progress: the file's name and how much of it has been sent, from 0 to 1. */
export interface FileUpload {
  name: string;
  fraction: number;
}

export interface FileDropProps
  extends Pick<DropZoneProps, "isDisabled" | "aria-label" | "aria-labelledby"> {
  /** Called with the files that passed the type and size checks; never with an empty list. */
  onSelect: (files: File[]) => void;
  /** Called with the files that were refused, after they are shown in the zone. */
  onReject?: (rejections: FileRejection[]) => void;
  /**
   * MIME types (`application/pdf`), MIME groups (`image/*`) or extensions (`.pdf`). Without it,
   * any type is accepted.
   */
  acceptedFileTypes?: readonly string[];
  /** The largest file accepted, in bytes. */
  maxFileSize?: number;
  /** Accept several files at once; by default one. */
  allowsMultiple?: boolean;
  /** Which types are accepted, in words ("PDF, Word or an image"). */
  description?: ReactNode;
  /** The upload in progress, if any. */
  upload?: FileUpload;
  /** An error from outside the zone, such as the server refusing the upload. */
  errorMessage?: ReactNode;
  /** Layout only. */
  className?: string;
}

/** Whether `file` matches one of `accepted`: a MIME type, a `type/*` group or an extension. */
export function acceptsType(file: File, accepted: readonly string[] | undefined): boolean {
  if (accepted === undefined || accepted.length === 0) return true;
  const name = file.name.toLowerCase();
  const type = file.type.toLowerCase();
  return accepted.some((entry) => {
    const pattern = entry.trim().toLowerCase();
    if (pattern.startsWith(".")) return name.endsWith(pattern);
    if (pattern.endsWith("/*")) return type !== "" && type.startsWith(pattern.slice(0, -1));
    return type === pattern;
  });
}

/** Splits `files` into those to hand on and those refused for their type or size. */
export function checkFiles(
  files: readonly File[],
  { acceptedFileTypes, maxFileSize }: Pick<FileDropProps, "acceptedFileTypes" | "maxFileSize">,
): { accepted: File[]; rejected: FileRejection[] } {
  const accepted: File[] = [];
  const rejected: FileRejection[] = [];
  for (const file of files) {
    if (!acceptsType(file, acceptedFileTypes)) rejected.push({ file, reason: "type" });
    else if (maxFileSize !== undefined && file.size > maxFileSize)
      rejected.push({ file, reason: "size" });
    else accepted.push(file);
  }
  return { accepted, rejected };
}

/** A byte count as the values of the `fileDrop.size` message: kilobytes and up, decimal units. */
export function sizeValues(bytes: number): MessageValues {
  if (bytes >= 1e9) return { unit: "GB", value: bytes / 1e9 };
  if (bytes >= 1e6) return { unit: "MB", value: bytes / 1e6 };
  return { unit: "kB", value: bytes / 1e3 };
}

/**
 * Hands chosen or dropped files to `onSelect` after checking their type and size, and lists the
 * refused ones with the reason. Dropping several files on a zone that takes one refuses them all.
 */
export function FileDrop({
  onSelect,
  onReject,
  acceptedFileTypes,
  maxFileSize,
  allowsMultiple = false,
  description,
  upload,
  errorMessage,
  isDisabled,
  className,
  ...props
}: FileDropProps) {
  const { t } = useStepclimbStrings();
  const [problems, setProblems] = useState<Problem[]>([]);
  const nextId = useRef(0);
  const carriesFiles = useRef(false);
  const show = (texts: string[]) =>
    setProblems(texts.map((text) => ({ id: nextId.current++, text })));
  const multiple = allowsMultiple ? "yes" : "no";

  function receive(files: File[]) {
    if (files.length === 0) return;
    if (!allowsMultiple && files.length > 1) {
      show([t("fileDrop.tooMany")]);
      return;
    }
    const { accepted, rejected } = checkFiles(files, { acceptedFileTypes, maxFileSize });
    show(
      rejected.map(({ file, reason }) =>
        reason === "type"
          ? t("fileDrop.wrongType", { name: file.name })
          : t("fileDrop.tooLarge", {
              name: file.name,
              size: t("fileDrop.size", sizeValues(maxFileSize ?? 0)),
            }),
      ),
    );
    if (rejected.length > 0) onReject?.(rejected);
    if (accepted.length > 0) onSelect(accepted);
  }

  return (
    // React Aria's drag types say which MIME types a drag carries, not whether any item is a file
    // (a dragged link and a dragged .txt file both read text/plain); the browser's dataTransfer
    // does. Read before the DropZone's own handlers, which ask `getDropOperation`.
    <div
      data-slot="file-drop-frame"
      className="contents"
      onDragEnterCapture={(event) => {
        carriesFiles.current = dragCarriesFiles(event.dataTransfer);
      }}
    >
      <DropZone
        data-slot="file-drop"
        aria-label={t("fileDrop.dropTarget")}
        {...props}
        isDisabled={isDisabled}
        getDropOperation={() => (carriesFiles.current ? "copy" : "cancel")}
        onDrop={async (event) => {
          const items = event.items.filter(isFileDropItem);
          receive(await Promise.all(items.map((item) => item.getFile())));
        }}
        className={styles.root({ className })}
      >
        <Icon icon={UploadSimpleIcon} size="lg" className={styles.icon()} />
        <Text data-slot="file-drop-prompt" slot="label" className={styles.prompt()}>
          {t("fileDrop.prompt", { multiple })}
        </Text>
        <FileTrigger
          acceptedFileTypes={acceptedFileTypes}
          allowsMultiple={allowsMultiple}
          onSelect={(list) => receive(list === null ? [] : [...list])}
        >
          <Button data-slot="file-drop-choose" isDisabled={isDisabled}>
            {t("fileDrop.choose", { multiple })}
          </Button>
        </FileTrigger>
        {description === undefined && maxFileSize === undefined ? null : (
          <p data-slot="file-drop-limits" className={styles.limits()}>
            {description}
            {description !== undefined && maxFileSize !== undefined ? " " : null}
            {maxFileSize === undefined
              ? null
              : t("fileDrop.limit", { size: t("fileDrop.size", sizeValues(maxFileSize)) })}
          </p>
        )}
        {upload === undefined ? null : (
          <ProgressBar
            data-slot="file-drop-progress"
            label={t("fileDrop.uploading", { name: upload.name })}
            value={upload.fraction * 100}
            className={styles.progress()}
          />
        )}
        {/* In the page from the first render, so a refusal is announced when it appears. */}
        <div data-slot="file-drop-errors" role="alert" className={styles.errors()}>
          {problems.map((problem) => (
            <FileDropError key={problem.id}>{problem.text}</FileDropError>
          ))}
          {errorMessage === undefined ? null : <FileDropError>{errorMessage}</FileDropError>}
        </div>
      </DropZone>
    </div>
  );
}

/**
 * Whether a drag carries at least one file. Safari lists no items while dragging files, only the
 * `Files` type.
 */
export function dragCarriesFiles(data: Pick<DataTransfer, "items" | "types"> | null): boolean {
  if (data === null) return false;
  return [...data.items].some((item) => item.kind === "file") || data.types.includes("Files");
}

function FileDropError({ children }: { children: ReactNode }) {
  return (
    <p data-slot="file-drop-error" className={styles.error()}>
      <Icon icon={WarningCircleIcon} className={styles.errorIcon()} />
      <span>{children}</span>
    </p>
  );
}
