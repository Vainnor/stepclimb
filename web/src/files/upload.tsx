// SPDX-License-Identifier: AGPL-3.0-or-later
// Uploading into the folder on screen (docs/specs/m1/06-files.md B3 to B7;
// 16-learning-ui-components.md B6, B20): FileDrop takes one or more files, which go up one after
// another, each under its own name, with its progress in the zone. A status line under the zone,
// a polite live region, says which file is uploading and how each one ended. The zone stays
// enabled, so focus stays on "Choose files"; a selection made while a batch runs is not sent, and
// the status line says so. The first refusal stops the rest: it is shown in the zone with the
// file's name, and the files after it are counted as not uploaded. Each stored file refreshes the
// folder. Leaving the folder aborts the batch. The server decides every refusal, storage that is
// not configured (503 `storage.not_configured`) included.
import { Code, FileDrop, type FileUpload, Text, useStepclimbStrings } from "@stepclimb/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { courseFilesKey, uploadCourseFile } from "./course-files";
import { type FilesProblem, filesProblem } from "./problems";

interface Refusal {
  problem: FilesProblem;
  /** Files chosen with the refused one that were not sent. */
  notSent: number;
}

export interface UploadZoneProps {
  courseId: string;
  folder: string;
  /** The folder as the page names it: its name, or "Top folder". */
  folderLabel: string;
}

function isAbort(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

/**
 * The signal of an AbortController made when the component mounts and aborted when it unmounts.
 * Read it when a batch starts, after mounting.
 */
function useUnmountSignal(): () => AbortSignal | undefined {
  const controller = useRef<AbortController>(undefined);
  useEffect(() => {
    const current = new AbortController();
    controller.current = current;
    return () => current.abort();
  }, []);
  return () => controller.current?.signal;
}

interface Send {
  file: File;
  signal: AbortSignal | undefined;
}

export function UploadZone({ courseId, folder, folderLabel }: UploadZoneProps) {
  const { t } = useStepclimbStrings();
  const queryClient = useQueryClient();
  const unmountSignal = useUnmountSignal();
  const [upload, setUpload] = useState<FileUpload>();
  const [status, setStatus] = useState("");
  const [refusal, setRefusal] = useState<Refusal>();
  const running = useRef(false);
  const send = useMutation({
    mutationFn: ({ file, signal }: Send) =>
      uploadCourseFile(
        courseId,
        folder,
        file,
        (fraction) => setUpload({ name: file.name, fraction }),
        signal,
      ),
  });

  async function uploadAll(files: File[]) {
    running.current = true;
    const signal = unmountSignal();
    setRefusal(undefined);
    for (const [index, file] of files.entries()) {
      setUpload({ name: file.name, fraction: 0 });
      setStatus(t("files.upload.uploading", { name: file.name }));
      try {
        const stored = await send.mutateAsync({ file, signal });
        setStatus(t("files.upload.done", { name: stored.name }));
      } catch (error) {
        if (isAbort(error)) return;
        setStatus("");
        setRefusal({
          problem: filesProblem(error, file.name, t),
          notSent: files.length - index - 1,
        });
        break;
      } finally {
        if (!signal?.aborted) {
          void queryClient.invalidateQueries({ queryKey: courseFilesKey(courseId) });
        }
      }
    }
    running.current = false;
    setUpload(undefined);
  }

  function onSelect(files: File[]) {
    if (running.current) {
      setStatus(t("files.upload.busy", { name: upload?.name ?? "" }));
      return;
    }
    void uploadAll(files);
  }

  return (
    <div className="flex flex-col gap-2">
      <FileDrop
        allowsMultiple
        onSelect={onSelect}
        upload={upload}
        description={t("files.upload.description", { folder: folderLabel })}
        errorMessage={refusal === undefined ? undefined : <RefusalText refusal={refusal} />}
      />
      {/* In the page from the first render, so each change is announced. */}
      <Text data-slot="files-upload-status" elementType="p" size="sm" tone="sub" role="status">
        {status}
      </Text>
    </div>
  );
}

function RefusalText({ refusal }: { refusal: Refusal }) {
  const { t } = useStepclimbStrings();
  const { problem, notSent } = refusal;
  return (
    <>
      {problem.message}
      {problem.code === undefined ? null : (
        <>
          {" "}
          <Code>{problem.code}</Code>
        </>
      )}
      {notSent > 0 ? <> {t("files.upload.notSent", { count: notSent })}</> : null}
    </>
  );
}
