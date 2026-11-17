// SPDX-License-Identifier: AGPL-3.0-or-later
// What VideoUploader says when an upload or a video fails (docs/specs/m1/08-uploaded-video.md B2,
// B4 to B7, API, D3, D8, D13, D15): one message per problem `code` the tus routes and the video's
// `error_code` can carry, each saying what happened and what to do. A code without a message of
// its own gets a general one and the code itself, so the person can report it.
import type { Strings } from "../strings/strings";

/** Why an upload stopped or a video failed. */
export type UploadProblem =
  /** The request got no answer: the network or the server is down. */
  | { kind: "unreachable" }
  /** A tus route refused the request; status 0 is an answer this client could not use. */
  | { kind: "refused"; status: number; code: string | undefined }
  /** The video failed on the server with `code` (B2). */
  | { kind: "video"; code: string }
  /** The video was deleted while it was being processed. */
  | { kind: "gone" };

/** What to tell the person: a sentence, and the code when the sentence is the general one. */
export interface ProblemText {
  message: string;
  code?: string;
}

/**
 * Whether the upload can go on from where it stopped once the cause has passed: no answer, a
 * stalled body (408), a rate limit (429) or a server-side failure (5xx). Chunks are stored whole
 * or not at all (D5), so resuming from the stored offset loses nothing.
 */
export function isResumable(problem: UploadProblem): boolean {
  if (problem.kind === "unreachable") return true;
  if (problem.kind !== "refused") return false;
  const { status } = problem;
  return status === 408 || status === 429 || status >= 500;
}

/** The message for a code a tus route answers, or undefined. */
function refusalMessage(code: string | undefined, name: string, t: Strings["t"]): string | undefined {
  switch (code) {
    case "storage.not_configured":
      return t("videoUploader.problem.storageNotConfigured");
    case "file.too_large":
      return t("videoUploader.problem.tooLarge", { name });
    case "entitlement.limit_reached":
      return t("videoUploader.problem.storageFull", { name });
    case "course.archived":
      return t("videoUploader.problem.archived");
    case "course.not_found":
      return t("videoUploader.problem.courseGone");
    case "permission.denied":
      return t("videoUploader.problem.forbidden");
    case "auth.unauthenticated":
      return t("videoUploader.problem.signedOut", { name });
    case "auth.csrf_refused":
      return t("videoUploader.problem.csrf");
    case "request.invalid":
      return t("videoUploader.problem.titleInvalid", { name });
    case "video_upload.not_found":
      return t("videoUploader.problem.uploadGone", { name });
    case "video.status_invalid":
      return t("videoUploader.problem.alreadyComplete", { name });
    case "video.upload_offset_mismatch":
    case "tus.offset_invalid":
    case "tus.length_mismatch":
      return t("videoUploader.problem.outOfStep", { name });
    case "file.uploads_busy":
      return t("videoUploader.problem.uploadsBusy");
    case "file.body_timeout":
      return t("videoUploader.problem.bodyTimeout");
    default:
      return undefined;
  }
}

/** The message for a video's `error_code` (B2), or undefined. */
function videoMessage(code: string, name: string, t: Strings["t"]): string | undefined {
  switch (code) {
    case "video.not_a_video":
      return t("videoUploader.problem.notAVideo", { name });
    case "video.probe_failed":
      return t("videoUploader.problem.probeFailed", { name });
    case "video.too_long":
      return t("videoUploader.problem.tooLong", { name });
    case "video.transcode_failed":
      return t("videoUploader.problem.transcodeFailed", { name });
    case "entitlement.limit_reached":
      return t("videoUploader.problem.planLimit", { name });
    default:
      return undefined;
  }
}

/** What to tell the person about `problem`, for the video or file `name`. */
export function problemText(problem: UploadProblem, name: string, t: Strings["t"]): ProblemText {
  switch (problem.kind) {
    case "unreachable":
      return { message: t("videoUploader.problem.unreachable") };
    case "gone":
      return { message: t("videoUploader.problem.videoGone", { name }) };
    case "video": {
      const known = videoMessage(problem.code, name, t);
      if (known !== undefined) return { message: known };
      return { message: t("videoUploader.problem.videoFailed"), code: problem.code };
    }
    case "refused": {
      const known = refusalMessage(problem.code, name, t);
      if (known !== undefined) return { message: known };
      if (problem.status >= 500) return { message: t("videoUploader.problem.serverFailed") };
      return {
        message: t("videoUploader.problem.unknown"),
        code: problem.code ?? `HTTP ${problem.status}`,
      };
    }
  }
}
