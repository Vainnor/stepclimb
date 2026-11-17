// SPDX-License-Identifier: AGPL-3.0-or-later
// What the files area says when a request fails (docs/specs/m1/06-files.md API;
// 16-learning-ui-components.md B20): one message per problem `code` the file routes return, each
// saying what happened and what to do. An unknown code gets a general message and the code itself,
// so the person can report it.
import type { Strings } from "@stepclimb/ui";
import { ApiError, isUnreachable } from "../api";

export interface FilesProblem {
  /** What happened and what to do. */
  message: string;
  /** The server's code, shown in mono, when the page has no message of its own for it. */
  code?: string;
}

/** The rule a 422 says the request broke: the first field error's code. */
function fieldCode(error: ApiError): string | undefined {
  return error.problem?.errors?.[0]?.code;
}

/** The message for a rule a name or folder broke (`06` B9). */
function ruleMessage(code: string | undefined, t: Strings["t"]): string | undefined {
  switch (code) {
    case "file.name_invalid":
      return t("files.problem.nameInvalid");
    case "file.name_too_long":
      return t("files.problem.nameTooLong");
    case "file.folder_invalid":
      return t("files.problem.folderInvalid");
    case "file.folder_too_long":
      return t("files.problem.folderTooLong");
    case "file.folder_too_deep":
      return t("files.problem.folderTooDeep");
    case "file.folder_root":
      return t("files.problem.folderRoot");
    default:
      return undefined;
  }
}

/** The message for a problem `code` the files area explains, or undefined. */
function codeMessage(error: ApiError, name: string, t: Strings["t"]): string | undefined {
  const problem = error.problem;
  switch (problem?.code) {
    case "storage.not_configured":
      return t("files.problem.storageNotConfigured");
    case "file.name_taken":
      return t("files.problem.nameTaken", { name });
    case "file.too_large":
      return t("files.problem.tooLarge", { name });
    case "file.uploads_busy":
      return t("files.problem.uploadsBusy");
    case "file.body_timeout":
      return t("files.problem.bodyTimeout", { name });
    case "file.in_use":
      return t("files.problem.inUse", { name });
    case "file.not_found":
      return t("files.problem.fileGone");
    case "folder.not_found":
      return t("files.problem.folderGone");
    case "course.archived":
      return t("files.problem.archived");
    case "course.not_found":
      return t("files.problem.courseGone");
    case "permission.denied":
      return t("files.problem.forbidden");
    case "entitlement.limit_reached":
      return t("files.problem.storageFull", { name });
    case "request.invalid":
      return ruleMessage(fieldCode(error), t);
    default:
      return undefined;
  }
}

/**
 * What to tell the person about `error`, a failed request about the file or folder `name`
 * (the uploaded file's name, the renamed file's or the folder's).
 */
export function filesProblem(error: unknown, name: string, t: Strings["t"]): FilesProblem {
  if (isUnreachable(error)) return { message: t("files.problem.unreachable") };
  if (!(error instanceof ApiError)) return { message: t("files.problem.failed") };
  const known = codeMessage(error, name, t);
  if (known !== undefined) return { message: known };
  return {
    message: t("files.problem.unknown"),
    code: error.problem?.code ?? `HTTP ${error.status}`,
  };
}

/** True when the course file or folder a request named is gone, so the list on screen is stale. */
export function isGone(error: unknown): boolean {
  if (!(error instanceof ApiError)) return false;
  const code = error.problem?.code;
  return code === "file.not_found" || code === "folder.not_found";
}
