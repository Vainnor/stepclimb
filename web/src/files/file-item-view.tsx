// SPDX-License-Identifier: AGPL-3.0-or-later
// A `file` item's content in the item view (docs/specs/m1/06-files.md B18, Q8). It asks the
// server for an inline signed URL and shows a PDF in a frame where the browser shows PDFs in the
// page, an image with `<img>`, and every other type, and a PDF where the browser does not show
// one, as a download only. The download button asks for an attachment URL each time it is
// pressed, so it never reuses an expired one. A locked item asks for nothing: the server would
// refuse it, and the item view explains the lock.
import type { components } from "@stepclimb/api-client";
import {
  Button,
  ErrorState,
  type HeadingLevel,
  Skeleton,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery } from "@tanstack/react-query";
import { ApiError } from "../api";
import { LoadError } from "../layouts/load-error";
import {
  browserShowsPdf,
  fileShow,
  inlineFileUrlQuery,
  itemFileUrl,
  type SignedUrl,
} from "./file-item";
import { fileSize } from "./file-table";

type Item = components["schemas"]["Item"];

export interface FileItemViewProps {
  item: Item;
  /** The level of a heading the view shows for a refusal. */
  level: HeadingLevel;
  /** Whether this browser shows PDFs in the page; read from the browser when omitted. */
  pdfViewer?: boolean;
}

export function FileItemView({ item, level, pdfViewer }: FileItemViewProps) {
  if (item.kind !== "file" || item.lock !== null) return null;
  return <FileContent item={item} level={level} pdfViewer={pdfViewer ?? browserShowsPdf()} />;
}

function FileContent({ item, level, pdfViewer }: Required<FileItemViewProps>) {
  const { t } = useStepclimbStrings();
  const url = useQuery(inlineFileUrlQuery(item.id));
  if (url.isPending) {
    return <Skeleton className="h-64 w-full" />;
  }
  if (url.isError) {
    if (problemCode(url.error) === "storage.not_configured") {
      return (
        <ErrorState title={t("fileItem.noStorage.title")} level={level}>
          {t("fileItem.noStorage.description")}
        </ErrorState>
      );
    }
    return <LoadError error={url.error} level={level} onRetry={() => void url.refetch()} />;
  }
  const signed = url.data;
  const show = fileShow(signed.mime, pdfViewer);
  return (
    <div data-slot="file-item" className="flex flex-col gap-4">
      {show === "pdf" ? (
        <iframe
          data-slot="file-preview"
          src={signed.url}
          title={t("fileItem.preview", { title: item.title })}
          className="h-[70vh] min-h-96 w-full rounded-md border border-line"
        />
      ) : null}
      {show === "image" ? (
        <img
          data-slot="file-image"
          src={signed.url}
          alt={item.title}
          className="max-h-[70vh] max-w-full self-start rounded-md border border-line"
        />
      ) : null}
      {show === "pdf-fallback" ? <Text>{t("fileItem.noPdfViewer")}</Text> : null}
      <Download itemId={item.id} file={signed} />
    </div>
  );
}

/** The download button, with the file's name and size, and its refusal in words. */
function Download({ itemId, file }: { itemId: string; file: SignedUrl }) {
  const { t } = useStepclimbStrings();
  const download = useMutation({
    mutationFn: () => itemFileUrl(itemId, "attachment"),
    // The answer is `Content-Disposition: attachment`, so the browser saves it and stays here.
    onSuccess: (signed) => globalThis.location.assign(signed.url),
  });
  return (
    <div data-slot="file-download" className="flex flex-col items-start gap-2">
      <Text>{t("fileItem.file", { name: file.name, size: fileSize(file.size, t) })}</Text>
      <Button isPending={download.isPending} onPress={() => download.mutate()}>
        {t("fileItem.download")}
      </Button>
      <div aria-live="polite">
        {download.isError ? <Text>{t("fileItem.downloadFailed")}</Text> : null}
      </div>
    </div>
  );
}

function problemCode(error: unknown): string | undefined {
  return error instanceof ApiError ? error.problem?.code : undefined;
}
