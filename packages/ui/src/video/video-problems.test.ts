// SPDX-License-Identifier: AGPL-3.0-or-later
// Every problem code of docs/specs/m1/08-uploaded-video.md that reaches VideoUploader has its own
// message; only codes the person cannot act on fall back to the general one with the code.
import { describe, expect, it } from "vitest";
import { uiMessages } from "../strings/catalog";
import { createStrings } from "../strings/strings";
import { isResumable, problemText } from "./video-problems";

const { t } = createStrings({ catalogs: [uiMessages], isDevelopment: true });

/** The codes the tus routes answer the uploader (B4, B5, D3, D8, D13, D15, API). */
const refusals: [number, string][] = [
  [503, "storage.not_configured"],
  [413, "file.too_large"],
  [403, "entitlement.limit_reached"],
  [409, "course.archived"],
  [404, "course.not_found"],
  [403, "permission.denied"],
  [401, "auth.unauthenticated"],
  [403, "auth.csrf_refused"],
  [422, "request.invalid"],
  [404, "video_upload.not_found"],
  [409, "video.status_invalid"],
  [409, "video.upload_offset_mismatch"],
  [503, "file.uploads_busy"],
  [408, "file.body_timeout"],
];

/** The `error_code`s a video fails with (B2). */
const failures = [
  "video.not_a_video",
  "video.probe_failed",
  "video.too_long",
  "video.transcode_failed",
  "entitlement.limit_reached",
];

describe("problemText", () => {
  it.each(refusals)("has a message of its own for %i %s", (status, code) => {
    const text = problemText({ kind: "refused", status, code }, "Week 1.mp4", t);
    expect(text.code).toBeUndefined();
    expect(text.message).not.toBe(t("videoUploader.problem.unknown"));
  });

  it.each(failures)("has a message of its own for the video failure %s", (code) => {
    const text = problemText({ kind: "video", code }, "Week 1.mp4", t);
    expect(text.code).toBeUndefined();
    expect(text.message).toContain("Week 1.mp4");
  });

  it("tells a full plan at creation from a plan limit during processing", () => {
    const refused = problemText(
      { kind: "refused", status: 403, code: "entitlement.limit_reached" },
      "a.mp4",
      t,
    );
    const failed = problemText({ kind: "video", code: "entitlement.limit_reached" }, "a.mp4", t);
    expect(refused.message).not.toBe(failed.message);
  });

  it.each([
    [400, "video.chunk_too_small"],
    [411, "file.length_required"],
    [415, "request.unsupported_media_type"],
    [400, "request.malformed"],
    [412, "tus.version_unsupported"],
  ])("asks for a reload and names the code for the protocol refusal %i %s", (status, code) => {
    expect(problemText({ kind: "refused", status, code }, "a", t)).toEqual({
      message: t("videoUploader.problem.protocol"),
      code,
    });
  });

  it("names the code it has no message for", () => {
    expect(problemText({ kind: "refused", status: 404, code: "org.not_found" }, "a", t)).toEqual({
      message: "The server refused the upload.",
      code: "org.not_found",
    });
    expect(problemText({ kind: "refused", status: 400, code: undefined }, "a", t)).toEqual({
      message: "The server refused the upload.",
      code: "HTTP 400",
    });
    expect(problemText({ kind: "video", code: "video.something_new" }, "a", t)).toEqual({
      message: "The video could not be prepared for playback.",
      code: "video.something_new",
    });
    expect(problemText({ kind: "refused", status: 500, code: undefined }, "a", t)).toEqual({
      message: "Something went wrong on the server. Try again in a moment.",
    });
  });
});

describe("isResumable", () => {
  it("is true for no answer, a stall, a rate limit and a server failure only", () => {
    expect(isResumable({ kind: "unreachable" })).toBe(true);
    for (const status of [408, 429, 500, 503]) {
      expect(isResumable({ kind: "refused", status, code: undefined })).toBe(true);
    }
    for (const status of [0, 400, 401, 403, 404, 409, 413]) {
      expect(isResumable({ kind: "refused", status, code: undefined })).toBe(false);
    }
    expect(isResumable({ kind: "video", code: "video.too_long" })).toBe(false);
    expect(isResumable({ kind: "gone" })).toBe(false);
  });
});
