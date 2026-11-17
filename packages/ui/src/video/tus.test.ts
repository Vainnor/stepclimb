// SPDX-License-Identifier: AGPL-3.0-or-later
// The tus client against the in-memory server of src/test/tus-server.ts
// (docs/specs/m1/08-uploaded-video.md B4 to B6, B22, D1, D3, D13, D15): creation, chunks of
// 16 MiB, resuming from the offset `HEAD` answers, a 409 offset mismatch, the 1 MiB smallest
// chunk, termination and refusals.
import { describe, expect, it } from "vitest";
import { ENDPOINT, MockTusServer } from "../test/tus-server";
import {
  base64Utf8,
  CHUNK_BYTES,
  checkedLength,
  createUpload,
  endUpload,
  MIN_CHUNK_BYTES,
  readOffset,
  sendChunk,
  sendFile,
  TusProblem,
  titleFromFileName,
} from "./tus";

const MIB = 1024 * 1024;
const signal = () => new AbortController().signal;

function video(size: number, name = "Week 1.mp4"): File {
  return new File([new Uint8Array(size)], name, { type: "video/mp4" });
}

/** Sends `file` to a new upload on `server` from `offset`, recording the stored offsets. */
async function upload(server: MockTusServer, file: File, chunkBytes?: number) {
  const created = await createUpload(server.send, ENDPOINT, file, signal());
  const stored: number[] = [];
  await sendFile(server.send, created.url, file, {
    offset: 0,
    chunkBytes,
    signal: signal(),
    onProgress: () => {},
    onStored: (offset) => stored.push(offset),
  });
  return { created, stored };
}

function decodeMetadata(header: string | null): Record<string, string> {
  const pairs = (header ?? "").split(",").map((pair) => pair.split(" "));
  return Object.fromEntries(
    pairs.map(([key = "", value = ""]) => [
      key,
      new TextDecoder().decode(Uint8Array.from(atob(value), (c) => c.charCodeAt(0))),
    ]),
  );
}

describe("createUpload", () => {
  it("sends the length and the name, and answers the upload's address and video", async () => {
    const server = new MockTusServer();
    const created = await createUpload(server.send, ENDPOINT, video(5 * MIB, "Café.mp4"), signal());
    expect(created).toEqual({ url: "http://localhost/api/v1/video-uploads/u1", videoId: "v1" });
    const [post] = server.received;
    expect(post?.method).toBe("POST");
    expect(post?.headers.get("tus-resumable")).toBe("1.0.0");
    expect(post?.headers.get("upload-length")).toBe(String(5 * MIB));
    expect(decodeMetadata(post?.headers.get("upload-metadata") ?? null)).toEqual({
      filename: "Café.mp4",
      title: "Café",
    });
  });

  it("gives a refusal's status and code", async () => {
    const server = new MockTusServer();
    server.overrides.push(
      () =>
        new Response(JSON.stringify({ code: "file.too_large", title: "x", status: 413 }), {
          status: 413,
          headers: { "content-type": "application/problem+json" },
        }),
    );
    const refused = createUpload(server.send, ENDPOINT, video(10), signal());
    await expect(refused).rejects.toEqual(new TusProblem(413, "file.too_large"));
  });

  it("refuses an answer without Location", async () => {
    const server = new MockTusServer();
    server.overrides.push(() => new Response("{}", { status: 201 }));
    await expect(createUpload(server.send, ENDPOINT, video(10), signal())).rejects.toMatchObject({
      status: 0,
      code: "tus.location_missing",
    });
  });
});

describe("sendFile", () => {
  it("sends chunks of 16 MiB and a shorter last one, each at the stored offset", async () => {
    const server = new MockTusServer();
    const file = video(2 * CHUNK_BYTES + 3 * MIB);
    const { stored } = await upload(server, file);
    expect(server.chunks().map((c) => [c.headers.get("upload-offset"), c.bodySize])).toEqual([
      ["0", CHUNK_BYTES],
      [String(CHUNK_BYTES), CHUNK_BYTES],
      [String(2 * CHUNK_BYTES), 3 * MIB],
    ]);
    expect(server.chunks().map((c) => c.headers.get("content-type"))).toEqual(
      Array(3).fill("application/offset+octet-stream"),
    );
    expect(stored).toEqual([CHUNK_BYTES, 2 * CHUNK_BYTES, file.size]);
    expect(server.videos.get("v1")?.status).toBe("processing");
  });

  it("resumes from the offset HEAD answers after a dropped connection", async () => {
    const server = new MockTusServer();
    const file = video(3 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    // The second chunk is stored, but its answer is lost.
    let chunk = 0;
    server.overrides.push((request) => {
      if (request.method !== "PATCH") return undefined;
      chunk += 1;
      if (chunk === 2) server.dropAfterStore = true;
      return undefined;
    });
    const options = { chunkBytes: MIB, signal: signal(), onProgress: () => {}, onStored: () => {} };
    await expect(sendFile(server.send, created.url, file, { ...options, offset: 0 })).rejects.toThrow(
      "Failed to fetch",
    );
    const stored = checkedLength(await readOffset(server.send, created.url, signal()), file.size);
    expect(stored).toEqual({ offset: 2 * MIB, length: 3 * MIB });
    await sendFile(server.send, created.url, file, { ...options, offset: stored.offset });
    expect(server.chunks().map((c) => c.headers.get("upload-offset"))).toEqual([
      "0",
      String(MIB),
      String(2 * MIB),
    ]);
    expect(server.uploads.get("u1")?.offset).toBe(file.size);
  });

  it("takes the stored offset after a 409 offset mismatch and goes on from there", async () => {
    const server = new MockTusServer();
    const file = video(3 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    // Another tab already sent the first two chunks.
    const stored = server.uploads.get("u1");
    if (stored) stored.offset = 2 * MIB;
    const offsets: number[] = [];
    await sendFile(server.send, created.url, file, {
      offset: 0,
      chunkBytes: MIB,
      signal: signal(),
      onProgress: () => {},
      onStored: (offset) => offsets.push(offset),
    });
    expect(server.received.map((r) => r.method)).toEqual(["POST", "PATCH", "HEAD", "PATCH"]);
    expect(server.chunks().at(-1)?.headers.get("upload-offset")).toBe(String(2 * MIB));
    expect(offsets).toEqual([2 * MIB, 3 * MIB]);
  });

  it("gives up after three offset mismatches in a row", async () => {
    const server = new MockTusServer();
    const file = video(2 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    server.overrides.push((request) =>
      request.method === "PATCH"
        ? new Response(JSON.stringify({ code: "video.upload_offset_mismatch", title: "x" }), {
            status: 409,
            headers: { "content-type": "application/problem+json" },
          })
        : undefined,
    );
    const sent = sendFile(server.send, created.url, file, {
      offset: 0,
      chunkBytes: MIB,
      signal: signal(),
      onProgress: () => {},
      onStored: () => {},
    });
    await expect(sent).rejects.toEqual(new TusProblem(409, "video.upload_offset_mismatch"));
    expect(server.chunks()).toHaveLength(4);
  });

  it("never sends a chunk under 1 MiB unless it completes the upload", async () => {
    const server = new MockTusServer();
    const file = video(MIB + 10);
    await upload(server, file, MIN_CHUNK_BYTES);
    expect(server.chunks().map((c) => c.bodySize)).toEqual([MIB, 10]);
    expect(server.uploads.get("u1")?.offset).toBe(file.size);
  });

  it("refuses chunk sizes under the server's 1 MiB minimum", async () => {
    const server = new MockTusServer();
    const file = video(3 * MIB);
    const send = sendFile(server.send, "http://localhost/api/v1/video-uploads/u1", file, {
      offset: 0,
      chunkBytes: MIN_CHUNK_BYTES - 1,
      signal: signal(),
      onProgress: () => {},
      onStored: () => {},
    });
    await expect(send).rejects.toThrow(RangeError);
    expect(server.received).toEqual([]);
  });

  it("is refused by the server for a short chunk that does not complete the upload", async () => {
    const server = new MockTusServer();
    const created = await createUpload(server.send, ENDPOINT, video(3 * MIB), signal());
    const short = sendChunk(server.send, created.url, 0, new Blob([new Uint8Array(512)]), () => {}, signal());
    await expect(short).rejects.toEqual(new TusProblem(400, "video.chunk_too_small"));
  });

  it("reports the bytes held plus those of the chunk in flight", async () => {
    const server = new MockTusServer();
    server.hold = true;
    const file = video(2 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    const progress: number[] = [];
    const sent = sendFile(server.send, created.url, file, {
      offset: 0,
      chunkBytes: MIB,
      signal: signal(),
      onProgress: (bytes) => progress.push(bytes),
      onStored: () => {},
    });
    await expect.poll(() => server.held).toBe(1);
    expect(progress).toEqual([MIB / 2]);
    server.hold = false;
    server.release();
    await sent;
    // The second chunk is not held, so it reports only its whole body sent.
    expect(progress).toEqual([MIB / 2, MIB, MIB, 2 * MIB, 2 * MIB]);
  });

  it("stops with an AbortError when aborted", async () => {
    const server = new MockTusServer();
    server.hold = true;
    const file = video(2 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    const controller = new AbortController();
    const sent = sendFile(server.send, created.url, file, {
      offset: 0,
      chunkBytes: MIB,
      signal: controller.signal,
      onProgress: () => {},
      onStored: () => {},
    });
    await expect.poll(() => server.held).toBe(1);
    controller.abort();
    await expect(sent).rejects.toMatchObject({ name: "AbortError" });
    expect(server.uploads.get("u1")?.offset).toBe(0);
  });

  it("refuses an answer whose offset is not where the chunk ended", async () => {
    const server = new MockTusServer();
    const file = video(2 * MIB);
    const created = await createUpload(server.send, ENDPOINT, file, signal());
    server.overrides.push((request) =>
      request.method === "PATCH"
        ? new Response(null, { status: 204, headers: { "Upload-Offset": "5" } })
        : undefined,
    );
    const sent = sendFile(server.send, created.url, file, {
      offset: 0,
      chunkBytes: MIB,
      signal: signal(),
      onProgress: () => {},
      onStored: () => {},
    });
    await expect(sent).rejects.toMatchObject({ status: 0, code: "tus.offset_invalid" });
  });
});

describe("endUpload", () => {
  it("ends the upload; it is gone afterwards", async () => {
    const server = new MockTusServer();
    const created = await createUpload(server.send, ENDPOINT, video(3 * MIB), signal());
    await endUpload(server.send, created.url, signal());
    expect(server.received.at(-1)?.method).toBe("DELETE");
    expect(server.received.at(-1)?.headers.get("tus-resumable")).toBe("1.0.0");
    await expect(readOffset(server.send, created.url, signal())).rejects.toEqual(
      new TusProblem(404, "video_upload.not_found"),
    );
  });
});

describe("helpers", () => {
  it("encodes metadata values as base64 of their UTF-8 bytes", () => {
    expect(base64Utf8("Week 1")).toBe("V2VlayAx");
    expect(base64Utf8("é")).toBe("w6k=");
  });

  it("titles a video after its file name without the extension, at most 200 characters", () => {
    expect(titleFromFileName("Week 1.mp4")).toBe("Week 1");
    expect(titleFromFileName("archive.tar.mov")).toBe("archive.tar");
    expect(titleFromFileName(".mp4")).toBe(".mp4");
    expect(titleFromFileName("noextension")).toBe("noextension");
    expect([...titleFromFileName(`${"é".repeat(250)}.mp4`)]).toHaveLength(200);
  });

  it("refuses a stored upload of another length", () => {
    expect(() => checkedLength({ offset: 0, length: 9 }, 10)).toThrow(TusProblem);
    expect(() => checkedLength({ offset: 11, length: 10 }, 10)).toThrow(TusProblem);
    expect(checkedLength({ offset: 4, length: 10 }, 10)).toEqual({ offset: 4, length: 10 });
  });
});
