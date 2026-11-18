// SPDX-License-Identifier: AGPL-3.0-or-later
// A tus 1.0.0 client for the subset Stepclimb's server speaks (docs/specs/m1/08-uploaded-video.md
// B4 to B6, D1 to D16): creation with `Upload-Length` and `Upload-Metadata`, `HEAD` for the
// stored offset, `PATCH` chunks of `application/offset+octet-stream`, and termination with
// `DELETE`. Each request goes out through the caller's `send`, so the web app sends it through
// `XMLHttpRequest` with the upload's progress and its own error for a request that got no answer.
//
// A chunk is stored whole or not at all (D5), so after any failure the upload resumes from the
// offset `HEAD` answers. Every chunk but the last is `CHUNK_BYTES` long, never under the server's
// 1 MiB minimum for a chunk that does not complete the upload (D15).

/** The tus protocol version the server speaks (D3). */
export const TUS_VERSION = "1.0.0";

/** The length of every chunk but the last (B22). */
export const CHUNK_BYTES = 16 * 1024 * 1024;

/** The shortest chunk the server takes unless it completes the upload (D15). */
export const MIN_CHUNK_BYTES = 1024 * 1024;

/** How many `409 video.upload_offset_mismatch` answers in a row the upload resynchronizes from. */
const MAX_OFFSET_RESYNCS = 3;

/**
 * Sends `request` with `body`, reporting the share of the body sent so far from 0 to 1, and gives
 * back the server's answer. It rejects with an `AbortError` when `signal` aborts, and with any
 * other error when the request got no answer.
 */
export type Send = (
  request: Request,
  body: Blob | null,
  onProgress: (fraction: number) => void,
  signal: AbortSignal,
) => Promise<Response>;

/**
 * An answer the server refused: its status and its problem `code`, when it sent one. An answer
 * this client cannot use (no `Location`, an offset that is not where the chunk ended) has status
 * 0 and a `tus.*` code of the client's own.
 */
export class TusProblem extends Error {
  readonly status: number;
  readonly code: string | undefined;

  constructor(status: number, code: string | undefined) {
    super(code === undefined ? `HTTP ${status}` : `${status} ${code}`);
    this.name = "TusProblem";
    this.status = status;
    this.code = code;
  }
}

/** True for the rejection of an aborted request. */
export function isAbort(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

/** The problem `code` of a refusal's problem+json body, if it has one. */
async function problemCode(response: Response): Promise<string | undefined> {
  const type = response.headers.get("content-type") ?? "";
  if (!type.toLowerCase().startsWith("application/problem+json")) return undefined;
  try {
    const body: unknown = await response.json();
    if (typeof body === "object" && body !== null && "code" in body) {
      return typeof body.code === "string" ? body.code : undefined;
    }
  } catch {
    // A body that is not JSON carries no code; the status still says what happened.
  }
  return undefined;
}

/** `response` when it is a success, or its refusal as a `TusProblem`. */
async function expectOk(response: Response): Promise<Response> {
  if (response.ok) return response;
  throw new TusProblem(response.status, await problemCode(response));
}

/** A header holding a byte count: digits only. */
function byteHeader(response: Response, name: string): number {
  const value = response.headers.get(name);
  if (value === null || !/^\d{1,16}$/.test(value)) {
    throw new TusProblem(0, "tus.header_invalid");
  }
  return Number(value);
}

/** UTF-8 text as base64, the form of an `Upload-Metadata` value. */
export function base64Utf8(text: string): string {
  let binary = "";
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/** The longest title the server stores (D2, `videos.title`). */
const MAX_TITLE_CHARS = 200;

/**
 * The title a video starts with: the file's name without its extension, at most 200 characters,
 * or the whole name when nothing is left without the extension.
 */
export function titleFromFileName(name: string): string {
  const dot = name.lastIndexOf(".");
  const stem = (dot > 0 ? name.slice(0, dot) : name).trim();
  return [...(stem === "" ? name.trim() : stem)].slice(0, MAX_TITLE_CHARS).join("");
}

function tusHeaders(extra: Record<string, string> = {}): Headers {
  return new Headers({ "Tus-Resumable": TUS_VERSION, ...extra });
}

const ignoreProgress = () => {};

/** A started upload: the address of its chunks and the video it fills (D1). */
export interface CreatedUpload {
  url: string;
  videoId: string;
}

/** The video id in a creation's `VideoUploadCreated` body. */
async function createdVideoId(response: Response): Promise<string> {
  const body: unknown = await response.json().catch(() => undefined);
  if (
    typeof body === "object" &&
    body !== null &&
    "video" in body &&
    typeof body.video === "object" &&
    body.video !== null &&
    "id" in body.video &&
    typeof body.video.id === "string"
  ) {
    return body.video.id;
  }
  throw new TusProblem(0, "tus.body_invalid");
}

/**
 * Starts an upload of `file` at `endpoint` (B4). The server answers `Location`, resolved against
 * `endpoint`, and the video it created.
 */
export async function createUpload(
  send: Send,
  endpoint: string,
  file: File,
  signal: AbortSignal,
): Promise<CreatedUpload> {
  const metadata = [
    `filename ${base64Utf8(file.name)}`,
    `title ${base64Utf8(titleFromFileName(file.name))}`,
  ].join(",");
  const headers = tusHeaders({ "Upload-Length": String(file.size), "Upload-Metadata": metadata });
  const request = new Request(endpoint, { method: "POST", headers });
  const response = await expectOk(await send(request, null, ignoreProgress, signal));
  const location = response.headers.get("location");
  if (location === null) throw new TusProblem(0, "tus.location_missing");
  return { url: new URL(location, endpoint).href, videoId: await createdVideoId(response) };
}

/** The upload's stored offset and its whole length (B5). */
export async function readOffset(
  send: Send,
  url: string,
  signal: AbortSignal,
): Promise<{ offset: number; length: number }> {
  const request = new Request(url, { method: "HEAD", headers: tusHeaders() });
  const response = await expectOk(await send(request, null, ignoreProgress, signal));
  return {
    offset: byteHeader(response, "upload-offset"),
    length: byteHeader(response, "upload-length"),
  };
}

/** Sends `chunk` at `offset` and gives back the offset the server now holds (B5). */
export async function sendChunk(
  send: Send,
  url: string,
  offset: number,
  chunk: Blob,
  onProgress: (fraction: number) => void,
  signal: AbortSignal,
): Promise<number> {
  const headers = tusHeaders({
    "Upload-Offset": String(offset),
    "Content-Type": "application/offset+octet-stream",
  });
  const request = new Request(url, { method: "PATCH", headers });
  const response = await expectOk(await send(request, chunk, onProgress, signal));
  return byteHeader(response, "upload-offset");
}

/** Ends the upload: the server deletes its parts and its video (B5, D13). */
export async function endUpload(send: Send, url: string, signal: AbortSignal): Promise<void> {
  const request = new Request(url, { method: "DELETE", headers: tusHeaders() });
  await expectOk(await send(request, null, ignoreProgress, signal));
}

export interface SendFileOptions {
  /** Where the server's stored bytes end; the first chunk starts here. */
  offset: number;
  /** Called with the bytes the server holds plus those of the chunk in flight. */
  onProgress: (sent: number) => void;
  /** Called with the offset the server holds after each stored chunk. */
  onStored: (offset: number) => void;
  signal: AbortSignal;
  /** Every chunk but the last; at least `MIN_CHUNK_BYTES`. */
  chunkBytes?: number;
}

/**
 * Sends `file` from `offset` to its end, a chunk at a time. A `409 video.upload_offset_mismatch`
 * (another tab or a lost answer moved the offset) is resolved by asking for the stored offset and
 * going on from there, up to three times in a row; any other refusal, an abort and a request that
 * got no answer reject.
 */
export async function sendFile(
  send: Send,
  url: string,
  file: Blob,
  { offset: start, onProgress, onStored, signal, chunkBytes = CHUNK_BYTES }: SendFileOptions,
): Promise<void> {
  if (chunkBytes < MIN_CHUNK_BYTES) throw new RangeError("chunkBytes is under MIN_CHUNK_BYTES");
  let offset = start;
  let resyncs = 0;
  while (offset < file.size) {
    const end = Math.min(offset + chunkBytes, file.size);
    const chunk = file.slice(offset, end);
    const at = offset;
    try {
      offset = checkedOffset(
        await sendChunk(send, url, at, chunk, (f) => onProgress(at + f * chunk.size), signal),
        end,
      );
      resyncs = 0;
    } catch (error) {
      if (!isOffsetMismatch(error) || resyncs >= MAX_OFFSET_RESYNCS) throw error;
      resyncs += 1;
      offset = checkedLength(await readOffset(send, url, signal), file.size).offset;
    }
    onStored(offset);
    onProgress(offset);
  }
}

function isOffsetMismatch(error: unknown): boolean {
  return (
    error instanceof TusProblem &&
    error.status === 409 &&
    error.code === "video.upload_offset_mismatch"
  );
}

/** The offset a chunk's answer gives, which must be where the chunk ended. */
function checkedOffset(offset: number, end: number): number {
  if (offset !== end) throw new TusProblem(0, "tus.offset_invalid");
  return offset;
}

/**
 * The stored offset of an upload, which must be of `size` bytes: an upload of another length is
 * not this file's.
 */
export function checkedLength(
  stored: { offset: number; length: number },
  size: number,
): { offset: number; length: number } {
  if (stored.length !== size || stored.offset > size) {
    throw new TusProblem(0, "tus.length_mismatch");
  }
  return stored;
}
