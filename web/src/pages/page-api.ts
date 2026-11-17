// SPDX-License-Identifier: AGPL-3.0-or-later
// The page editor's routes (docs/specs/m1/07-pages-and-the-editor.md B3, B5, B10): the current
// version with its body, saving a body against the version it was based on, and uploading an
// image into the course's `/_page-images` folder (spec 06 B3). The server decides every refusal;
// these carry its answer.
import type { components } from "@stepclimb/api-client";
import type { PageDoc, UploadedImage } from "@stepclimb/ui/editor";
import { queryOptions } from "@tanstack/react-query";
import { ApiError, api, type Problem, unwrap } from "../api";

export type PageSummary = components["schemas"]["PageSummary"];
export type PageVersion = components["schemas"]["PageVersion"];

/** The folder page images go into by default (B10). */
export const PAGE_IMAGES_FOLDER = "/_page-images";

/** The version an author edits: the page's current one, with its body. */
export interface EditablePage {
  version: Omit<PageVersion, "body">;
  body: PageDoc;
}

/**
 * The page's current version with its body. Every save and restore makes the version it writes
 * current and numbers it after every other (B3, B5), so the newest version is the current one.
 */
async function loadEditablePage(pageId: string): Promise<EditablePage> {
  const params = { path: { page_id: pageId } };
  const list = unwrap(
    await api.GET("/api/v1/pages/{page_id}/versions", {
      params: { ...params, query: { limit: 1 } },
    }),
  );
  const newest = list.items[0];
  if (!newest) throw new Error("a page has at least one version");
  const version = unwrap(
    await api.GET("/api/v1/pages/{page_id}/versions/{version_id}", {
      params: { path: { page_id: pageId, version_id: newest.id } },
    }),
  );
  const { body, ...summary } = version;
  return { version: summary, body: body as PageDoc };
}

export const editablePageQuery = (pageId: string) =>
  queryOptions({
    queryKey: ["pages", pageId, "editable"],
    queryFn: () => loadEditablePage(pageId),
    // The editor holds the author's work; a refetch must not replace it under them.
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });

/** Saves `body` as the page's new current version, refused when `base` is no longer current. */
export async function savePageBody(
  pageId: string,
  body: PageDoc,
  base: string,
): Promise<PageSummary> {
  return unwrap(
    await api.PUT("/api/v1/pages/{page_id}/body", {
      params: { path: { page_id: pageId } },
      // openapi-fetch's request type turns the schema's `"type": "null"` (a cell's `colwidth`)
      // into `undefined`, which no body with `null`, the server's own stored form, satisfies.
      // The fixtures shared with the server's tests (crates/stepclimb-core/tests/fixtures/
      // page_doc/) pin the body's shape instead.
      body: { body: body as never, base_version_id: base },
    }),
  );
}

/**
 * The name an uploaded image is stored under: the file's own name after a random prefix, so two
 * images called `diagram.png` do not collide in the folder (spec 06 B9: a name is unique in its
 * folder and at most 255 characters, without `/` or `\`).
 */
export function pageImageName(name: string): string {
  const prefix = Array.from(crypto.getRandomValues(new Uint8Array(4)), (b) =>
    b.toString(16).padStart(2, "0"),
  ).join("");
  const plain = [...name.replace(/[/\\\p{Cc}\p{Cf}]/gu, "")].slice(0, 200).join("").trim();
  return `${prefix}-${plain === "" || plain === "." || plain === ".." ? "image" : plain}`;
}

function problemOf(text: string): Problem | undefined {
  try {
    const parsed: unknown = JSON.parse(text);
    return typeof parsed === "object" && parsed !== null && "code" in parsed
      ? (parsed as Problem)
      : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Uploads `file` into the course's page image folder with `XMLHttpRequest`, which reports how
 * much of the body has been sent; `fetch` does not. The answer is the server's sniffed type, which
 * the editor checks before inserting the image (B10a).
 */
export function uploadPageImage(
  courseId: string,
  file: File,
  onProgress: (fraction: number) => void,
): Promise<UploadedImage> {
  const query = new URLSearchParams({ folder: PAGE_IMAGES_FOLDER, name: pageImageName(file.name) });
  const url = `/api/v1/courses/${encodeURIComponent(courseId)}/files?${query}`;
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    request.open("POST", url);
    request.setRequestHeader("Content-Type", "application/octet-stream");
    request.upload.onprogress = (event) => {
      if (event.lengthComputable && event.total > 0) onProgress(event.loaded / event.total);
    };
    request.onerror = () => reject(new Error("the server did not answer"));
    request.onload = () => {
      if (request.status === 201) {
        const created = JSON.parse(request.responseText) as components["schemas"]["CourseFile"];
        resolve({ course_file_id: created.id, mime: created.mime });
      } else {
        reject(new ApiError(request.status, problemOf(request.responseText)));
      }
    };
    request.send(file);
  });
}
