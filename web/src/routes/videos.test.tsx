// SPDX-License-Identifier: AGPL-3.0-or-later
// The video panel in jsdom (docs/specs/m1/08-uploaded-video.md B1, B2, B22, T14) against a stubbed
// server: the list with each video's status, the course's permissions deciding the uploader, an
// upload through the tus routes that refreshes the list, the list read again while a video
// processes, and the refused and missing states.

import { resumeKey } from "@stepclimb/ui";
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Course } from "../files/course-files";
import {
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";
import { stubXhr } from "../test/xhr";
import { readVideoStatus, type Video, videoUploadOwner } from "../videos/videos";

const courseId = "01900000-0000-7000-8000-0000000000c1";
const PATH = `/c/${courseId}/videos`;
const COURSE = `/api/v1/courses/${courseId}`;
const LIST = `/api/v1/courses/${courseId}/videos`;
const CREATE = `/api/v1/courses/${courseId}/video-uploads`;
const UPLOAD = "/api/v1/video-uploads/01900000-0000-7000-8000-0000000000u1";

const AUTHOR = ["course.content_edit", "course.view", "course.view_all"];
const VIEWER = ["course.view", "course.view_all"];

function courseFixture(permissions: string[], status: Course["status"] = "draft"): Course {
  return {
    id: courseId,
    account_id: "01900000-0000-7000-8000-0000000000a0",
    code: "GND-101",
    title: "Ground school",
    status,
    template: false,
    permissions,
    settings: { home: { kind: "modules" } },
    start_at: null,
    end_at: null,
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
  };
}

function videoFixture(n: number, title: string, status: string, error_code?: string): Video {
  return {
    id: `01900000-0000-7000-8000-00000000000${n}`,
    source: "upload",
    title,
    status,
    error_code: error_code ?? null,
    duration_s: null,
    chapters: [],
    created_at: "2026-11-02T12:00:00Z",
    updated_at: "2026-11-02T12:00:00Z",
  };
}

const ME = meFixture();
const OWNER = videoUploadOwner(ME);
const OTHER_OWNER = `${ME.organization.id}.01900000-0000-7000-8000-000000000099`;

/** An unfinished upload of `name` in this course, as VideoUploader keeps it. */
function resumeEntry(name: string) {
  return {
    scope: courseId,
    fingerprint: "[]",
    url: `${globalThis.location.origin}/api/v1/video-uploads/${name}`,
    videoId: `v-${name}`,
    name,
    size: 3_000_000,
    offset: 1_000_000,
    savedAt: Date.now(),
  };
}

const ready = videoFixture(1, "Week 1", "ready");
const failed = videoFixture(2, "Week 2", "failed", "video.too_long");

function server(
  permissions: string[] = AUTHOR,
  videos: () => Video[] = () => [ready, failed],
  more: Record<string, Handler> = {},
) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture()),
    [`GET ${COURSE}`]: () => json(courseFixture(permissions)),
    [`GET ${LIST}`]: () => json({ items: videos(), next_cursor: null }),
    ...more,
  });
}

const videoRows = () => [...document.querySelectorAll('[data-slot="video-row"]')];

/** The list's rows, once it has loaded. */
async function rows(): Promise<Element[]> {
  await waitFor(() => expect(videoRows().length).toBeGreaterThan(0));
  return videoRows();
}

beforeEach(() => stubMatchMedia(true));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  localStorage.clear();
});

describe("the video panel", () => {
  it("lists the course's videos with their status and why one failed", async () => {
    server();
    renderApp(PATH);
    expect(await screen.findByRole("heading", { level: 1, name: "Videos" })).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { level: 2, name: "Ground school" }),
    ).toBeInTheDocument();
    const [first, second] = await rows();
    expect(first).toHaveTextContent("Week 1");
    expect(first).toHaveTextContent("Ready · Added Nov 2, 2026");
    expect(second).toHaveTextContent("Failed · Added Nov 2, 2026");
    expect(second).toHaveTextContent("Week 2 is longer than this server accepts.");
    expect(screen.getByRole("button", { name: "Choose file" })).toBeInTheDocument();
    expect(document.title).toBe("Videos · Example Training");
  });

  it("offers no uploader without course.content_edit", async () => {
    server(VIEWER, () => []);
    renderApp(PATH);
    expect(await screen.findByText("This course has no videos.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose file" })).toBeNull();
  });

  it("offers no uploader in an archived course and says why", async () => {
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET ${COURSE}`]: () => json(courseFixture(AUTHOR, "archived")),
      [`GET ${LIST}`]: () => json({ items: [], next_cursor: null }),
    });
    renderApp(PATH);
    expect(
      await screen.findByText("This course is archived, so its videos cannot be changed."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose file" })).toBeNull();
  });

  it("says in words that the panel is for staff when the list is refused", async () => {
    server(VIEWER, () => [], {
      [`GET ${LIST}`]: () => problem(403, "permission.denied", "Forbidden"),
    });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { name: "You cannot open these videos" }),
    ).toBeInTheDocument();
  });

  it("says when the course cannot be found", async () => {
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET ${COURSE}`]: () => problem(404, "course.not_found", "Not Found"),
    });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { name: "This course could not be found" }),
    ).toBeInTheDocument();
  });

  it("uploads through the tus routes and refreshes the list", async () => {
    const created = videoFixture(3, "Week 3", "uploading");
    let videos = [ready];
    const fetch = server(AUTHOR, () => videos, {
      [`POST ${CREATE}`]: () =>
        new Response(JSON.stringify({ upload_id: "u1", video: created }), {
          status: 201,
          headers: { Location: UPLOAD, "Tus-Resumable": "1.0.0" },
        }),
      [`PATCH ${UPLOAD}`]: () => {
        videos = [ready, { ...created, status: "processing" }];
        return new Response(null, {
          status: 204,
          headers: { "Upload-Offset": "2048", "Tus-Resumable": "1.0.0" },
        });
      },
    });
    const xhr = stubXhr();
    renderApp(PATH);
    await screen.findByRole("button", { name: "Choose file" });
    const input = document.querySelector('[data-slot="file-drop"] input[type="file"]');
    if (!(input instanceof HTMLInputElement)) throw new Error("no file input");
    await userEvent.upload(
      input,
      new File([new Uint8Array(2048)], "Week 3.mp4", { type: "video/mp4" }),
    );
    expect(await screen.findByText("Week 3.mp4: processing", { selector: "span" })).toBeVisible();
    expect(xhr.sent.map((s) => [s.method, new URL(s.url).pathname])).toEqual([
      ["POST", CREATE],
      ["PATCH", UPLOAD],
    ]);
    expect(xhr.sent[0]?.headers).toMatchObject({
      "tus-resumable": "1.0.0",
      "upload-length": "2048",
    });
    expect(xhr.sent[1]?.headers).toMatchObject({
      "upload-offset": "0",
      "content-type": "application/offset+octet-stream",
    });
    expect(xhr.sent[1]?.body).toBeInstanceOf(Blob);
    expect((xhr.sent[1]?.body as Blob | undefined)?.size).toBe(2048);
    await waitFor(() => expect(videoRows()).toHaveLength(2));
    expect(videoRows()[1]).toHaveTextContent("Processing");
    expect(requestsTo(fetch, LIST).length).toBeGreaterThanOrEqual(2);
  });

  it("reads the list again every 10 s while a video is processing", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let status = "processing";
    const fetch = server(VIEWER, () => [{ ...ready, status }]);
    renderApp(PATH);
    const [row] = await rows();
    expect(row).toHaveTextContent("Processing");
    const before = requestsTo(fetch, LIST).length;
    status = "ready";
    await act(() => vi.advanceTimersByTimeAsync(10_000));
    await waitFor(() => expect(within(row as HTMLElement).getByText(/Ready/)).toBeInTheDocument());
    expect(requestsTo(fetch, LIST).length).toBe(before + 1);
    await act(() => vi.advanceTimersByTimeAsync(30_000));
    expect(requestsTo(fetch, LIST).length).toBe(before + 1);
  });
});

describe("unfinished uploads in this browser (spec 08 D30)", () => {
  beforeEach(() => {
    localStorage.setItem(resumeKey(OWNER), JSON.stringify([resumeEntry("Mine.mp4")]));
    localStorage.setItem(resumeKey(OTHER_OWNER), JSON.stringify([resumeEntry("Theirs.mp4")]));
  });

  it("lists the signed-in user's only, not another user's in the same course", async () => {
    server();
    renderApp(PATH);
    expect(await screen.findByText("Mine.mp4: 33% uploaded")).toBeInTheDocument();
    expect(screen.queryByText(/Theirs\.mp4/)).toBeNull();
  });

  it("are cleared by an explicit sign-out, leaving another user's", async () => {
    let signedOut = false;
    server(AUTHOR, () => [], {
      "GET /api/v1/me": () =>
        signedOut ? problem(401, "auth.unauthenticated", "Unauthorized") : json(ME),
      "POST /auth/sign-out": () => {
        signedOut = true;
        return new Response(null, { status: 204 });
      },
    });
    renderApp(PATH);
    await screen.findByText("Mine.mp4: 33% uploaded");
    await userEvent.click(screen.getByRole("button", { name: "Avery Example, account menu" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Sign out" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(localStorage.getItem(resumeKey(OWNER))).toBeNull();
    expect(localStorage.getItem(resumeKey(OTHER_OWNER))).not.toBeNull();
  });

  it("are kept when the session expires", async () => {
    let expired = false;
    server(AUTHOR, () => [], {
      "GET /api/v1/me": () =>
        expired ? problem(401, "auth.unauthenticated", "Unauthorized") : json(ME),
    });
    const app = renderApp(PATH);
    await screen.findByText("Mine.mp4: 33% uploaded");
    expired = true;
    await act(async () => {
      await app.context.queryClient.refetchQueries({ queryKey: ["me"] }).catch(() => undefined);
    });
    expect(await screen.findByRole("heading", { level: 1, name: "Sign in" })).toBeInTheDocument();
    expect(localStorage.getItem(resumeKey(OWNER))).not.toBeNull();
  });
});

describe("readVideoStatus", () => {
  it("answers the status, null for a video that is gone, and throws on other refusals", async () => {
    const id = ready.id;
    let answer: () => Response = () => json(failed);
    stubServer({ [`GET /api/v1/videos/${id}`]: () => answer() });
    const signal = new AbortController().signal;
    expect(await readVideoStatus(id, signal)).toEqual({
      status: "failed",
      error_code: "video.too_long",
    });
    answer = () => problem(404, "video.not_found", "Not Found");
    expect(await readVideoStatus(id, signal)).toBeNull();
    answer = () => problem(403, "permission.denied", "Forbidden");
    await expect(readVideoStatus(id, signal)).rejects.toMatchObject({ status: 403 });
  });
});
