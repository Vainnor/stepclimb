// SPDX-License-Identifier: AGPL-3.0-or-later
// The upload transport (files/course-files.ts `sendWithProgress`): progress, the answer as a
// Response, no answer as an UnreachableError, and an abort that aborts the request and rejects
// with an AbortError, as `fetch` does.
import { afterEach, describe, expect, it, vi } from "vitest";
import { UnreachableError } from "../api";
import { json, stubServer } from "../test/app";
import { stubXhr } from "../test/xhr";
import { sendWithProgress } from "./course-files";

afterEach(() => vi.unstubAllGlobals());

const request = () =>
  new Request("http://localhost:3000/api/v1/courses/c/files?name=a", {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
  });

describe("sendWithProgress", () => {
  it("reports progress and answers with the server's response", async () => {
    stubServer({ "POST /api/v1/courses/c/files": () => json({ id: "f" }, 201) });
    const xhr = stubXhr();
    xhr.hold = true;
    const progress = vi.fn();
    const answer = sendWithProgress(request(), new Blob(["abcd"]), progress);
    const [sent] = xhr.sent;
    sent?.progress(1, 4);
    expect(progress).toHaveBeenCalledWith(0.25);
    await sent?.finish();
    const response = await answer;
    expect(response.status).toBe(201);
    expect(await response.json()).toEqual({ id: "f" });
    expect(sent?.headers).toMatchObject({ "content-type": "application/octet-stream" });
  });

  it("rejects with an UnreachableError when no answer comes", async () => {
    stubServer({});
    stubXhr();
    await expect(
      sendWithProgress(request(), new Blob(["a"]), () => undefined),
    ).rejects.toBeInstanceOf(UnreachableError);
  });

  it("aborts the request and rejects with an AbortError when the signal aborts", async () => {
    stubServer({ "POST /api/v1/courses/c/files": () => json({ id: "f" }, 201) });
    const xhr = stubXhr();
    xhr.hold = true;
    const controller = new AbortController();
    const answer = sendWithProgress(request(), new Blob(["a"]), () => undefined, controller.signal);
    controller.abort();
    await expect(answer).rejects.toMatchObject({ name: "AbortError" });
    expect(xhr.sent[0]?.aborted).toBe(true);
  });

  it("sends nothing for a signal already aborted", async () => {
    const xhr = stubXhr();
    const controller = new AbortController();
    controller.abort();
    await expect(
      sendWithProgress(request(), new Blob(["a"]), () => undefined, controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(xhr.sent).toHaveLength(0);
  });
});
