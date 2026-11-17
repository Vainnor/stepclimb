// SPDX-License-Identifier: AGPL-3.0-or-later
// VideoUploader against the in-memory tus server (docs/specs/m1/08-uploaded-video.md B22, T14):
// upload, pause and resume, resume after a reload, a lost connection, cancel, processing then
// ready or failed, the refusals' messages and the polite progress announcements.
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { ENDPOINT, MockTusServer, memoryStorage } from "../test/tus-server";
import { RESUME_KEY, type ResumeEntry } from "./resume-store";
import { MIN_CHUNK_BYTES } from "./tus";
import { type GetVideo, POLL_MS } from "./use-video-upload";
import { VideoUploader, type VideoUploaderProps } from "./video-uploader";

const MIB = MIN_CHUNK_BYTES;

function video(size = 3 * MIB, name = "Week 1.mp4"): File {
  return new File([new Uint8Array(size)], name, { type: "video/mp4", lastModified: 1_000 });
}

interface Setup {
  server: MockTusServer;
  storage: Storage;
  onChange: Mock<() => void>;
  getVideo: Mock<GetVideo>;
}

function setup(): Setup {
  const server = new MockTusServer();
  const getVideo = vi.fn<GetVideo>(async (id) => server.videos.get(id) ?? null);
  return { server, storage: memoryStorage(), onChange: vi.fn<() => void>(), getVideo };
}

function renderUploader(s: Setup, props: Partial<VideoUploaderProps> = {}) {
  return renderWithStrings(
    <VideoUploader
      endpoint={ENDPOINT}
      storageKey="c1"
      send={s.server.send}
      getVideo={s.getVideo}
      onChange={s.onChange}
      storage={s.storage}
      chunkBytes={MIB}
      {...props}
    />,
  );
}

function fileInput(): HTMLInputElement {
  const input = document.querySelector('input[type="file"]');
  if (!(input instanceof HTMLInputElement)) throw new Error("no file input");
  return input;
}

async function choose(file: File) {
  await userEvent.upload(fileInput(), file);
}

const card = () => document.querySelector('[data-slot="video-upload"]');
const inCard = () => within(card() as HTMLElement);
const phase = () => card()?.getAttribute("data-phase");
const status = () => document.querySelector('[data-slot="video-uploader-status"]')?.textContent;
const alertText = () => within(card() as HTMLElement).getByRole("alert").textContent;

function stored(storage: Storage): ResumeEntry[] {
  return JSON.parse(storage.getItem(RESUME_KEY) ?? "[]") as ResumeEntry[];
}

afterEach(() => {
  vi.useRealTimers();
});

describe("VideoUploader", () => {
  it("uploads a video in chunks, then shows it processing", async () => {
    const s = setup();
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("processing"));
    expect(inCard().getByText("Week 1.mp4: processing")).toBeInTheDocument();
    expect(s.server.chunks().map((c) => c.headers.get("upload-offset"))).toEqual([
      "0",
      String(MIB),
      String(2 * MIB),
    ]);
    expect(s.server.videos.get("v1")?.status).toBe("processing");
    // Created, then complete.
    expect(s.onChange).toHaveBeenCalledTimes(2);
    expect(stored(s.storage)).toEqual([]);
    expect(status()).toBe("Week 1.mp4: processing");
  });

  it("pauses and resumes from the offset the server holds; focus stays on the button", async () => {
    const s = setup();
    renderUploader(s);
    s.server.hold = true;
    await choose(video());
    await waitFor(() => expect(s.server.held).toBe(1));
    const toggle = screen.getByRole("button", { name: "Pause" });
    await userEvent.click(toggle);
    expect(phase()).toBe("paused");
    expect(toggle).toHaveAccessibleName("Resume");
    expect(toggle).toHaveFocus();
    expect(s.server.uploads.get("u1")?.offset).toBe(0);

    s.server.hold = false;
    await userEvent.click(toggle);
    await waitFor(() => expect(phase()).toBe("processing"));
    expect(s.server.received.map((r) => r.method)).toEqual([
      "POST",
      "PATCH",
      "HEAD",
      "PATCH",
      "PATCH",
      "PATCH",
    ]);
    expect(s.server.chunks().at(1)?.headers.get("upload-offset")).toBe("0");
    // The buttons are gone; focus moved to the upload's name, not to the page.
    expect(inCard().getByText("Week 1.mp4: processing")).toHaveFocus();
  });

  it("resumes after a reload when the same file is chosen again", async () => {
    const s = setup();
    const file = video();
    const first = renderUploader(s);
    let patches = 0;
    s.server.overrides.push((request) => {
      if (request.method === "PATCH") patches += 1;
      if (patches === 2) s.server.hold = true;
      return undefined;
    });
    await choose(file);
    await waitFor(() => expect(s.server.held).toBe(1));
    expect(stored(s.storage)).toMatchObject([{ url: expect.stringContaining("u1"), offset: MIB }]);
    first.unmount();
    s.server.overrides = [];
    s.server.hold = false;

    renderUploader(s);
    expect(screen.getByRole("heading", { name: "Unfinished uploads" })).toBeInTheDocument();
    expect(screen.getByText("Week 1.mp4: 33% uploaded")).toBeInTheDocument();
    await choose(file);
    await waitFor(() => expect(phase()).toBe("processing"));
    const after = s.server.received.slice(-3);
    expect(after.map((r) => [r.method, r.headers.get("upload-offset")])).toEqual([
      ["HEAD", null],
      ["PATCH", String(MIB)],
      ["PATCH", String(2 * MIB)],
    ]);
    expect(s.server.received.filter((r) => r.method === "POST")).toHaveLength(1);
    expect(screen.queryByRole("heading", { name: "Unfinished uploads" })).toBeNull();
    expect(stored(s.storage)).toEqual([]);
  });

  it("starts again when the stored upload no longer exists", async () => {
    const s = setup();
    const file = video();
    s.storage.setItem(
      RESUME_KEY,
      JSON.stringify([
        {
          scope: "c1",
          fingerprint: JSON.stringify([file.name, file.size, file.type, file.lastModified]),
          url: "http://localhost/api/v1/video-uploads/gone",
          videoId: "v0",
          name: file.name,
          size: file.size,
          offset: MIB,
          savedAt: Date.now(),
        },
      ]),
    );
    renderUploader(s);
    s.server.hold = true;
    await choose(file);
    await waitFor(() => expect(s.server.held).toBe(1));
    expect(s.server.received.map((r) => r.method)).toEqual(["HEAD", "POST", "PATCH"]);
    expect(
      screen.getByText(
        "The earlier upload of Week 1.mp4 had ended, so this one started from the beginning.",
      ),
    ).toBeInTheDocument();
    expect(stored(s.storage).map((e) => e.url)).toEqual([
      "http://localhost/api/v1/video-uploads/u1",
    ]);
  });

  it("forgets stored uploads older than a day", () => {
    const s = setup();
    s.storage.setItem(
      RESUME_KEY,
      JSON.stringify([
        {
          scope: "c1",
          fingerprint: "[]",
          url: "http://localhost/api/v1/video-uploads/old",
          videoId: "v0",
          name: "Old.mp4",
          size: 10,
          offset: 5,
          savedAt: Date.now() - 24 * 60 * 60 * 1000 - 1,
        },
        {
          scope: "c2",
          fingerprint: "[]",
          url: "http://localhost/api/v1/video-uploads/other",
          videoId: "v9",
          name: "Other course.mp4",
          size: 10,
          offset: 5,
          savedAt: Date.now(),
        },
      ]),
    );
    renderUploader(s);
    expect(screen.queryByRole("heading", { name: "Unfinished uploads" })).toBeNull();
  });

  it("stops on a lost connection and goes on by itself when the browser is back online", async () => {
    const s = setup();
    renderUploader(s);
    let patches = 0;
    s.server.overrides.push((request) => {
      if (request.method !== "PATCH") return undefined;
      patches += 1;
      return patches === 2 ? "drop" : undefined;
    });
    await choose(video());
    await waitFor(() => expect(phase()).toBe("interrupted"));
    expect(alertText()).toBe("The connection to the server was lost.");
    expect(
      screen.getByText(
        "The upload goes on by itself when the connection is back, or select Resume.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument();
    act(() => {
      globalThis.dispatchEvent(new Event("online"));
    });
    await waitFor(() => expect(phase()).toBe("processing"));
    expect(s.server.uploads.get("u1")?.offset).toBe(3 * MIB);
  });

  it("cancels: the server ends the upload and nothing is kept", async () => {
    const s = setup();
    renderUploader(s);
    s.server.hold = true;
    await choose(video());
    await waitFor(() => expect(s.server.held).toBe(1));
    await userEvent.click(screen.getByRole("button", { name: "Cancel upload" }));
    await waitFor(() => expect(phase()).toBe("cancelled"));
    expect(s.server.received.at(-1)?.method).toBe("DELETE");
    expect(s.server.uploads.size).toBe(0);
    expect(stored(s.storage)).toEqual([]);
    expect(inCard().getByText("Upload of Week 1.mp4 canceled")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
  });

  it("says when the upload finished before the cancel reached the server", async () => {
    const s = setup();
    renderUploader(s);
    s.server.hold = true;
    s.server.overrides.push((request) =>
      request.method === "DELETE"
        ? new Response(JSON.stringify({ code: "video.status_invalid", title: "x" }), {
            status: 409,
            headers: { "content-type": "application/problem+json" },
          })
        : undefined,
    );
    await choose(video());
    await waitFor(() => expect(s.server.held).toBe(1));
    await userEvent.click(screen.getByRole("button", { name: "Cancel upload" }));
    await waitFor(() => expect(phase()).toBe("processing"));
    expect(alertText()).toBe(
      "Week 1.mp4 has finished uploading and is being processed, so its upload can no longer be canceled.",
    );
  });

  it("reads the video every 10 s until it is ready", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const s = setup();
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("processing"));
    expect(s.getVideo).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(POLL_MS));
    expect(s.getVideo).toHaveBeenCalledTimes(1);
    expect(phase()).toBe("processing");
    s.server.videos.set("v1", { status: "ready", error_code: null });
    await act(() => vi.advanceTimersByTimeAsync(POLL_MS));
    expect(s.getVideo).toHaveBeenCalledTimes(2);
    expect(phase()).toBe("ready");
    expect(inCard().getByText("Week 1.mp4 is ready")).toBeInTheDocument();
    expect(s.onChange).toHaveBeenCalledTimes(3);
    await act(() => vi.advanceTimersByTimeAsync(3 * POLL_MS));
    expect(s.getVideo).toHaveBeenCalledTimes(2);
  });

  it("shows why a video failed, and keeps reading through a failed read", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const s = setup();
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("processing"));
    s.getVideo.mockRejectedValueOnce(new Error("offline"));
    await act(() => vi.advanceTimersByTimeAsync(POLL_MS));
    expect(
      screen.getByText(
        "The server could not be asked whether the video is ready. Checking again in 10 seconds.",
      ),
    ).toBeInTheDocument();
    s.server.videos.set("v1", { status: "failed", error_code: "video.not_a_video" });
    await act(() => vi.advanceTimersByTimeAsync(POLL_MS));
    expect(phase()).toBe("failed");
    expect(alertText()).toBe(
      "Week 1.mp4 is not a video Stepclimb can read. Upload an MP4, MOV, WebM or MKV file.",
    );
  });

  it("says when the video was deleted while it was processed", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const s = setup();
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("processing"));
    s.server.videos.delete("v1");
    await act(() => vi.advanceTimersByTimeAsync(POLL_MS));
    expect(alertText()).toBe("Week 1.mp4 was deleted while it was being processed.");
  });

  it("shows a refused creation's message and sends no chunk", async () => {
    const s = setup();
    s.server.overrides.push(
      () =>
        new Response(JSON.stringify({ code: "storage.not_configured", title: "x" }), {
          status: 503,
          headers: { "content-type": "application/problem+json" },
        }),
    );
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("failed"));
    expect(alertText()).toBe(
      "Video storage is not set up on this server. Ask whoever runs Stepclimb for your organization to set it up.",
    );
    expect(s.server.chunks()).toEqual([]);
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
  });

  it("forgets an upload the server no longer has", async () => {
    const s = setup();
    renderUploader(s);
    let patches = 0;
    s.server.overrides.push((request) => {
      if (request.method !== "PATCH") return undefined;
      patches += 1;
      if (patches === 2) s.server.uploads.clear();
      return undefined;
    });
    await choose(video());
    await waitFor(() => expect(phase()).toBe("failed"));
    expect(alertText()).toBe(
      "The upload of Week 1.mp4 no longer exists: it was ended, or removed after a day without progress. Upload the video again.",
    );
    expect(stored(s.storage)).toEqual([]);
  });

  it("names the code of a refusal the page caused", async () => {
    const s = setup();
    s.server.overrides.push(
      () =>
        new Response(JSON.stringify({ code: "tus.version_unsupported", title: "x" }), {
          status: 412,
          headers: { "content-type": "application/problem+json" },
        }),
    );
    renderUploader(s);
    await choose(video());
    await waitFor(() => expect(phase()).toBe("failed"));
    expect(alertText()).toBe(
      "This page sent the upload in a form the server does not accept. Reload the page and choose the file again; if it happens again, report this code: tus.version_unsupported",
    );
  });

  it("takes one upload at a time", async () => {
    const s = setup();
    renderUploader(s);
    s.server.hold = true;
    await choose(video());
    await waitFor(() => expect(s.server.held).toBe(1));
    await choose(video(2 * MIB, "Week 2.mp4"));
    expect(
      screen.getByText(
        "Week 1.mp4 is still uploading. Wait for it to finish, or cancel it, before choosing another video.",
      ),
    ).toBeInTheDocument();
    expect(s.server.received.filter((r) => r.method === "POST")).toHaveLength(1);
  });

  it("announces progress politely in steps of 10 %", async () => {
    const s = setup();
    renderUploader(s);
    s.server.hold = true;
    await choose(video(10 * MIB));
    await waitFor(() => expect(s.server.held).toBe(1));
    // Half of the first 1 MiB chunk is 5 %: still the 0 % step.
    expect(status()).toBe("Week 1.mp4: 0% uploaded");
    expect(document.querySelector('[data-slot="video-uploader-status"]')).toHaveAttribute(
      "role",
      "status",
    );
    s.server.release();
    await waitFor(() => expect(s.server.held).toBe(1));
    expect(status()).toBe("Week 1.mp4: 10% uploaded");
  });

  it("discards an unfinished upload from before a reload", async () => {
    const s = setup();
    const file = video();
    const first = renderUploader(s);
    s.server.hold = true;
    let patches = 0;
    s.server.overrides.push((request) => {
      if (request.method === "PATCH") patches += 1;
      s.server.hold = patches >= 2;
      return undefined;
    });
    await choose(file);
    await waitFor(() => expect(s.server.held).toBe(1));
    first.unmount();
    cleanup();
    renderUploader(s);
    await userEvent.click(screen.getByRole("button", { name: "Discard the upload of Week 1.mp4" }));
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Unfinished uploads" })).toBeNull(),
    );
    expect(s.server.received.at(-1)?.method).toBe("DELETE");
    expect(s.server.uploads.size).toBe(0);
    expect(stored(s.storage)).toEqual([]);
  });

  it("renders only catalog text", async () => {
    const s = setup();
    s.server.hold = true;
    const { container } = renderWithStrings(
      <VideoUploader
        endpoint={ENDPOINT}
        storageKey="c1"
        send={s.server.send}
        getVideo={s.getVideo}
        storage={s.storage}
        chunkBytes={MIB}
      />,
      { marked: true },
    );
    await choose(video(3 * MIB, "«caller»"));
    await waitFor(() => expect(s.server.held).toBe(1));
    expect(unmarkedText(container)).toEqual([]);
  });
});
