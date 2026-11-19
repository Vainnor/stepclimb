// SPDX-License-Identifier: AGPL-3.0-or-later
// A `file` item's file through the public API (docs/specs/m1/06-files.md B14, B16, B18): a signed
// URL for the item, and how the item view shows what it serves. The server decides whether the
// caller may have the URL (the item's visibility and lock); this only carries its answer.
import type { components } from "@stepclimb/api-client";
import { queryOptions } from "@tanstack/react-query";
import { api, unwrap } from "../api";

export type SignedUrl = components["schemas"]["SignedUrl"];
export type Disposition = components["schemas"]["Disposition"];

/**
 * How long an inline URL is reused: a minute short of the server's 10-minute lifetime (Q7), so
 * a view opened later asks for a new one. A preview already loaded keeps showing what it loaded.
 */
const INLINE_URL_STALE_MS = 9 * 60 * 1000;

/** A new signed URL of the `file` item `itemId`. */
export async function itemFileUrl(itemId: string, disposition: Disposition): Promise<SignedUrl> {
  return unwrap(
    await api.POST("/api/v1/items/{item_id}/file-url", {
      params: { path: { item_id: itemId } },
      body: { disposition },
    }),
  );
}

/** The inline URL the item view shows the file from. */
export function inlineFileUrlQuery(itemId: string) {
  return queryOptions({
    queryKey: ["items", itemId, "file-url", "inline"],
    queryFn: () => itemFileUrl(itemId, "inline"),
    staleTime: INLINE_URL_STALE_MS,
    gcTime: INLINE_URL_STALE_MS,
    // A new URL would reload the preview and lose the reader's place.
    refetchOnWindowFocus: false,
    retry: false,
  });
}

/** The image types the server serves inline (B17), which the item view shows with `<img>`. */
const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

/**
 * How the item view shows a file of type `mime` (B18, Q8): a PDF in a frame when the browser
 * shows PDFs in the page (`pdfViewer`, from `navigator.pdfViewerEnabled`), otherwise a message
 * with the download; an image inline; anything else as a download only.
 */
export type FileShow = "pdf" | "pdf-fallback" | "image" | "download";

export function fileShow(mime: string, pdfViewer: boolean): FileShow {
  if (mime === "application/pdf") return pdfViewer ? "pdf" : "pdf-fallback";
  if (IMAGE_TYPES.has(mime)) return "image";
  return "download";
}

/** Whether this browser shows PDFs in the page; `false` where it does not say. */
export function browserShowsPdf(): boolean {
  return globalThis.navigator?.pdfViewerEnabled === true;
}
