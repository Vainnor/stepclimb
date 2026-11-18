// SPDX-License-Identifier: AGPL-3.0-or-later
// Test and story support: an in-memory server for the tus subset of
// docs/specs/m1/08-uploaded-video.md B4 to B7 and D3, D6, D13, D15, behind a `Send`. It checks
// `Tus-Resumable`, answers creation with `Location` and the video, `HEAD` with the offset, takes a
// `PATCH` only at the stored offset (409 `video.upload_offset_mismatch` otherwise), refuses a chunk
// under 1 MiB that does not complete the upload (400 `video.chunk_too_small`) and one past the
// length (413), and ends an upload with `DELETE` unless it is complete (409
// `video.status_invalid`, D8). A test can hold chunks, drop the connection
// before or after a chunk is stored, or answer a request itself.
import { MIN_CHUNK_BYTES, type Send, TUS_VERSION } from "../video/tus";
import type { VideoStatus } from "../video/use-video-upload";

export const ENDPOINT = "http://localhost/api/v1/courses/c1/video-uploads";
const UPLOADS = "/api/v1/video-uploads/";

export interface ServerUpload {
  length: number;
  offset: number;
  videoId: string;
}

/** A request the server got: its method, path, headers and body length. */
export interface Received {
  method: string;
  path: string;
  headers: Headers;
  bodySize: number;
}

/** A test's own answer to a request; `"drop"` rejects as a lost connection. */
export type Override = (request: Request) => Response | "drop" | undefined;

function problem(status: number, code: string): Response {
  return new Response(JSON.stringify({ type: "about:blank", title: code, status, code }), {
    status,
    headers: { "content-type": "application/problem+json" },
  });
}

function tus(status: number, headers: Record<string, string> = {}, body?: string): Response {
  return new Response(body ?? null, {
    status,
    headers: { "Tus-Resumable": TUS_VERSION, ...headers },
  });
}

function aborted(): DOMException {
  return new DOMException("The request was aborted.", "AbortError");
}

export class MockTusServer {
  readonly uploads = new Map<string, ServerUpload>();
  readonly videos = new Map<string, VideoStatus>();
  readonly received: Received[] = [];
  /** Answers tried before the server's own, first to last; one returning undefined passes. */
  overrides: Override[] = [];
  /** While true, a chunk reports half its body sent and waits for `release`. */
  hold = false;
  /** Lose the answer of the next chunk after storing it. */
  dropAfterStore = false;
  private waiting: (() => void)[] = [];
  private nextId = 1;

  /** Lets every held chunk go on. */
  release(): void {
    const waiting = this.waiting;
    this.waiting = [];
    for (const go of waiting) go();
  }

  /** How many chunks are waiting for `release`; an aborted one stops waiting. */
  get held(): number {
    return this.waiting.length;
  }

  /** The `PATCH` requests so far. */
  chunks(): Received[] {
    return this.received.filter((r) => r.method === "PATCH");
  }

  readonly send: Send = async (request, body, onProgress, signal) => {
    if (signal.aborted) throw aborted();
    const path = new URL(request.url).pathname;
    this.received.push({
      method: request.method,
      path,
      headers: request.headers,
      bodySize: body?.size ?? 0,
    });
    for (const override of this.overrides) {
      const answer = override(request);
      if (answer === "drop") throw new TypeError("Failed to fetch");
      if (answer !== undefined) return answer;
    }
    if (request.headers.get("tus-resumable") !== TUS_VERSION) {
      return problem(412, "tus.version_unsupported");
    }
    if (request.method === "POST") return this.create(request);
    const upload = this.uploads.get(path.slice(UPLOADS.length));
    if (!path.startsWith(UPLOADS) || upload === undefined) {
      return problem(404, "video_upload.not_found");
    }
    if (request.method === "HEAD") {
      return tus(200, {
        "Upload-Offset": String(upload.offset),
        "Upload-Length": String(upload.length),
      });
    }
    if (request.method === "DELETE") {
      // A complete upload's video is processing: the server refuses to end it (D8).
      if (upload.offset === upload.length) return problem(409, "video.status_invalid");
      this.uploads.delete(path.slice(UPLOADS.length));
      this.videos.delete(upload.videoId);
      return tus(204);
    }
    return this.append(request, upload, body, onProgress, signal);
  };

  private create(request: Request): Response {
    const length = Number(request.headers.get("upload-length"));
    if (!Number.isInteger(length) || length <= 0) return problem(400, "request.malformed");
    const id = `u${this.nextId}`;
    const videoId = `v${this.nextId}`;
    this.nextId += 1;
    this.uploads.set(id, { length, offset: 0, videoId });
    this.videos.set(videoId, { status: "uploading", error_code: null });
    const video = { id: videoId, status: "uploading" };
    return tus(
      201,
      { Location: `${UPLOADS}${id}`, "content-type": "application/json" },
      JSON.stringify({ upload_id: id, video }),
    );
  }

  private async append(
    request: Request,
    upload: ServerUpload,
    body: Blob | null,
    onProgress: (fraction: number) => void,
    signal: AbortSignal,
  ): Promise<Response> {
    if (request.headers.get("content-type") !== "application/offset+octet-stream") {
      return problem(415, "request.unsupported_media_type");
    }
    const size = body?.size ?? 0;
    if (Number(request.headers.get("upload-offset")) !== upload.offset) {
      return problem(409, "video.upload_offset_mismatch");
    }
    const remaining = upload.length - upload.offset;
    if (size > remaining) return problem(413, "file.too_large");
    if (size > 0 && size < remaining && size < MIN_CHUNK_BYTES) {
      return problem(400, "video.chunk_too_small");
    }
    if (this.hold) {
      onProgress(0.5);
      await new Promise<void>((resolve, reject) => {
        this.waiting.push(resolve);
        signal.addEventListener(
          "abort",
          () => {
            this.waiting = this.waiting.filter((go) => go !== resolve);
            reject(aborted());
          },
          { once: true },
        );
      });
    }
    onProgress(1);
    upload.offset += size;
    if (upload.offset === upload.length) {
      this.videos.set(upload.videoId, { status: "processing", error_code: null });
    }
    if (this.dropAfterStore) {
      this.dropAfterStore = false;
      throw new TypeError("Failed to fetch");
    }
    return tus(204, { "Upload-Offset": String(upload.offset) });
  }
}

/** A local storage that lives in memory, for one test. */
export function memoryStorage(): Storage {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    clear: () => items.clear(),
    getItem: (key) => items.get(key) ?? null,
    key: (index) => [...items.keys()][index] ?? null,
    removeItem: (key) => {
      items.delete(key);
    },
    setItem: (key, value) => {
      items.set(key, value);
    },
  };
}
