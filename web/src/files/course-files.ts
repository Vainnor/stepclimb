// SPDX-License-Identifier: AGPL-3.0-or-later
// A course's files through the public API (docs/specs/m1/06-files.md B3, B10 to B12): the course,
// a folder's entries a page at a time, a folder's sub-folders for the tree, upload with progress,
// rename or move, folder rename and delete. The server decides every refusal; these carry its
// answer.
import type { components } from "@stepclimb/api-client";
import { queryOptions } from "@tanstack/react-query";
import { api, UnreachableError, unwrap } from "../api";

export type Course = components["schemas"]["Course"];
export type CourseFile = components["schemas"]["CourseFile"];
export type FolderEntry = components["schemas"]["FolderEntry"];
type FolderPage = components["schemas"]["Page_FolderEntry"];

/** The most entries one listing request asks for: the API's maximum. */
const TREE_PAGE_LIMIT = 200;

/** Every query of one course's files starts with this key, so one invalidation reaches them all. */
export function courseFilesKey(courseId: string) {
  return ["courses", courseId, "files"] as const;
}

export function courseQuery(courseId: string) {
  return queryOptions({
    queryKey: ["courses", courseId],
    queryFn: async (): Promise<Course> =>
      unwrap(
        await api.GET("/api/v1/courses/{course_id}", { params: { path: { course_id: courseId } } }),
      ),
  });
}

async function listFolder(courseId: string, folder: string, cursor?: string, limit?: number) {
  const query = {
    folder,
    ...(cursor === undefined ? {} : { cursor }),
    ...(limit ? { limit } : {}),
  };
  return unwrap(
    await api.GET("/api/v1/courses/{course_id}/files", {
      params: { path: { course_id: courseId }, query },
    }),
  );
}

/** One page of `folder`'s entries: sub-folders first, then files, each by name. */
export function folderPageQuery(courseId: string, folder: string, cursor: string | undefined) {
  return queryOptions({
    queryKey: [...courseFilesKey(courseId), "page", folder, cursor ?? null],
    queryFn: (): Promise<FolderPage> => listFolder(courseId, folder, cursor),
  });
}

/**
 * The paths of `folder`'s sub-folders. The listing puts sub-folders before files, so pages are
 * read only until one holds a file or there is no next page.
 */
export function subfoldersQuery(courseId: string, folder: string) {
  return queryOptions({
    queryKey: [...courseFilesKey(courseId), "subfolders", folder],
    queryFn: async (): Promise<string[]> => {
      const paths: string[] = [];
      let cursor: string | undefined;
      for (;;) {
        const page = await listFolder(courseId, folder, cursor, TREE_PAGE_LIMIT);
        for (const entry of page.items) if (entry.kind === "folder") paths.push(entry.path);
        const sawFile = page.items.some((entry) => entry.kind === "file");
        if (sawFile || page.next_cursor === null) return paths;
        cursor = page.next_cursor;
      }
    },
  });
}

export async function updateCourseFile(
  id: string,
  change: { folder?: string; name?: string },
): Promise<CourseFile> {
  const params = { path: { course_file_id: id } };
  return unwrap(await api.PATCH("/api/v1/course-files/{course_file_id}", { params, body: change }));
}

export async function renameFolder(courseId: string, from: string, to: string): Promise<number> {
  const params = { path: { course_id: courseId } };
  const renamed = await api.POST("/api/v1/courses/{course_id}/folders/rename", {
    params,
    body: { from, to },
  });
  return unwrap(renamed).moved;
}

export async function deleteCourseFile(id: string): Promise<void> {
  const params = { path: { course_file_id: id } };
  unwrap(await api.DELETE("/api/v1/course-files/{course_file_id}", { params }));
}

/** How far an upload's body has gone, from 0 to 1. */
export type UploadProgress = (fraction: number) => void;

/**
 * Uploads `file` into `folder` under its own name. The body is the file's bytes as they are; the
 * server reads its type from them and counts them against `Content-Length`, which the browser
 * sets from the file. `fetch` reports no upload progress, so the request goes out through
 * `XMLHttpRequest` (`sendWithProgress`), still built and answered through the API client.
 * Aborting `signal` stops the upload and rejects with an `AbortError`.
 */
export async function uploadCourseFile(
  courseId: string,
  folder: string,
  file: File,
  onProgress: UploadProgress,
  signal?: AbortSignal,
): Promise<CourseFile> {
  const result = await api.POST("/api/v1/courses/{course_id}/files", {
    params: { path: { course_id: courseId }, query: { folder, name: file.name } },
    // The schema types a raw body as a list of bytes; the serializer hands over the File itself.
    body: [],
    bodySerializer: () => file,
    headers: { "Content-Type": "application/octet-stream" },
    fetch: (request: Request) => sendWithProgress(request, file, onProgress, signal),
  });
  return unwrap(result);
}

/** The headers of an `XMLHttpRequest`'s answer. */
function responseHeaders(xhr: XMLHttpRequest): Headers {
  const headers = new Headers();
  for (const line of xhr
    .getAllResponseHeaders()
    .trim()
    .split(/[\r\n]+/)) {
    const at = line.indexOf(":");
    if (at > 0) headers.append(line.slice(0, at).trim(), line.slice(at + 1).trim());
  }
  return headers;
}

/** Statuses whose response has no body. */
const NULL_BODY = new Set([204, 205, 304]);

/**
 * Sends `request` with `body` through `XMLHttpRequest`, reporting the body's progress, and gives
 * back the answer as a `Response`. A request that gets no answer rejects with an
 * `UnreachableError`, as every other API request does (`fetchOrUnreachable`). Aborting `signal`
 * aborts the request and rejects with an `AbortError`, as `fetch` does.
 */
export function sendWithProgress(
  request: Request,
  body: Blob,
  onProgress: UploadProgress,
  signal?: AbortSignal,
): Promise<Response> {
  return new Promise((resolve, reject) => {
    const aborted = () => new DOMException("The upload was aborted.", "AbortError");
    if (signal?.aborted) {
      reject(aborted());
      return;
    }
    const xhr = new XMLHttpRequest();
    const onAbort = () => xhr.abort();
    signal?.addEventListener("abort", onAbort, { once: true });
    const settle = () => signal?.removeEventListener("abort", onAbort);
    xhr.open(request.method, request.url);
    for (const [key, value] of request.headers) xhr.setRequestHeader(key, value);
    xhr.upload.addEventListener("progress", (event) => {
      if (event.lengthComputable && event.total > 0) onProgress(event.loaded / event.total);
    });
    xhr.addEventListener("load", () => {
      settle();
      const status = xhr.status;
      resolve(
        new Response(NULL_BODY.has(status) ? null : xhr.responseText, {
          status,
          headers: responseHeaders(xhr),
        }),
      );
    });
    xhr.addEventListener("error", () => {
      settle();
      reject(new UnreachableError(new Error("upload failed")));
    });
    xhr.addEventListener("abort", () => {
      settle();
      reject(aborted());
    });
    xhr.send(body);
  });
}
