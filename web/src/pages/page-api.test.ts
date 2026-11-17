// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../api";
import { pageImageName, uploadPageImage } from "./page-api";

describe("pageImageName", () => {
  it("prefixes the file's name so two uploads of the same name do not collide", () => {
    const name = pageImageName("diagram.png");
    expect(name).toMatch(/^[0-9a-f]{8}-diagram\.png$/);
    expect(pageImageName("diagram.png")).not.toBe(name);
  });

  it("drops what a folder name may not hold and keeps it under 255 characters", () => {
    expect(pageImageName("a/b\\c\u0000‮.png")).toMatch(/^[0-9a-f]{8}-abc\.png$/);
    expect(pageImageName("..")).toMatch(/^[0-9a-f]{8}-image$/);
    expect([...pageImageName("x".repeat(400))].length).toBeLessThanOrEqual(255);
  });
});

/** A stand-in XMLHttpRequest that answers `status` with `body` and reports one progress step. */
function stubXhr(status: number, body: unknown) {
  const sent: { method: string; url: string; headers: Record<string, string>; body: unknown }[] =
    [];
  class FakeXhr {
    status = 0;
    responseText = "";
    upload: { onprogress: ((e: ProgressEvent) => void) | null } = { onprogress: null };
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    private request = {
      method: "",
      url: "",
      headers: {} as Record<string, string>,
      body: null as unknown,
    };
    open(method: string, url: string) {
      this.request.method = method;
      this.request.url = url;
    }
    setRequestHeader(name: string, value: string) {
      this.request.headers[name] = value;
    }
    send(payload: unknown) {
      this.request.body = payload;
      sent.push(this.request);
      this.upload.onprogress?.({ lengthComputable: true, loaded: 1, total: 2 } as ProgressEvent);
      this.status = status;
      this.responseText = JSON.stringify(body);
      this.onload?.();
    }
  }
  vi.stubGlobal("XMLHttpRequest", FakeXhr);
  return sent;
}

afterEach(() => vi.unstubAllGlobals());

describe("uploadPageImage", () => {
  const file = new File(["png"], "diagram.png", { type: "image/png" });

  it("posts the file into the course's page image folder and answers its id and sniffed type", async () => {
    const sent = stubXhr(201, { id: "f1", mime: "image/png" });
    const progress: number[] = [];
    const uploaded = await uploadPageImage("course-1", file, (f) => progress.push(f));
    expect(uploaded).toEqual({ course_file_id: "f1", mime: "image/png" });
    expect(progress).toEqual([0.5]);
    const [request] = sent;
    expect(request?.method).toBe("POST");
    const url = new URL(request?.url ?? "", "https://stepclimb.test");
    expect(url.pathname).toBe("/api/v1/courses/course-1/files");
    expect(url.searchParams.get("folder")).toBe("/_page-images");
    expect(url.searchParams.get("name")).toMatch(/-diagram\.png$/);
    expect(request?.body).toBe(file);
  });

  it("rejects with the server's problem", async () => {
    stubXhr(413, {
      type: "about:blank",
      title: "x",
      status: 413,
      detail: "x",
      code: "file.too_large",
    });
    const error = await uploadPageImage("course-1", file, () => undefined).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).problem?.code).toBe("file.too_large");
  });
});
