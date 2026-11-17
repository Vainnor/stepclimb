// SPDX-License-Identifier: AGPL-3.0-or-later
// Uploading into the folder on screen (docs/specs/m1/06-files.md B3 to B7;
// 16-learning-ui-components.md B6, B20): FileDrop takes one or more files, which go up one after
// another, each under its own name, with its progress in the zone. The first refusal stops the
// rest: it is shown in the zone with the file's name, and the files after it are counted as not
// uploaded. Each stored file refreshes the folder and is announced in a toast. The server decides
// every refusal, storage that is not configured (503 `storage.not_configured`) included.
import { Code, FileDrop, type FileUpload, useStepclimbStrings } from "@stepclimb/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useAppContext } from "../context";
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

export function UploadZone({ courseId, folder, folderLabel }: UploadZoneProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  const [upload, setUpload] = useState<FileUpload>();
  const [refusal, setRefusal] = useState<Refusal>();
  const send = useMutation({
    mutationFn: (file: File) =>
      uploadCourseFile(courseId, folder, file, (fraction) =>
        setUpload({ name: file.name, fraction }),
      ),
  });

  async function uploadAll(files: File[]) {
    setRefusal(undefined);
    for (const [index, file] of files.entries()) {
      setUpload({ name: file.name, fraction: 0 });
      try {
        const stored = await send.mutateAsync(file);
        toasts.add({ tone: "success", title: t("files.upload.done", { name: stored.name }) });
      } catch (error) {
        setRefusal({
          problem: filesProblem(error, file.name, t),
          notSent: files.length - index - 1,
        });
        break;
      } finally {
        void queryClient.invalidateQueries({ queryKey: courseFilesKey(courseId) });
      }
    }
    setUpload(undefined);
  }

  return (
    <FileDrop
      allowsMultiple
      isDisabled={upload !== undefined}
      onSelect={(files) => void uploadAll(files)}
      upload={upload}
      description={t("files.upload.description", { folder: folderLabel })}
      errorMessage={refusal === undefined ? undefined : <RefusalText refusal={refusal} />}
    />
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
