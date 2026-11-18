// SPDX-License-Identifier: AGPL-3.0-or-later
// What the page editor says for each refusal of a save or an image upload
// (docs/specs/m1/07-pages-and-the-editor.md B3, B7, B10, B10a; 06-files.md B4): the server's
// code, the message for it, and, for a refusal of the body, the top-level block its pointer names.
import type { Strings } from "@stepclimb/ui";
import { blockIndexOf, type EditorProblem } from "@stepclimb/ui/editor";
import { ApiError, isUnreachable, type Problem } from "../api";

/** The body's first member the server refused, from a 422's `errors`. */
function bodyError(problem: Problem | undefined) {
  return problem?.errors?.find(
    (error) => error.field === "/body" || error.field.startsWith("/body/"),
  );
}

/** A refused save, as the editor shows it. */
export function saveProblem(error: unknown, t: Strings["t"]): EditorProblem {
  if (isUnreachable(error)) return { message: t("pageEdit.problem.unreachable") };
  const problem = error instanceof ApiError ? error.problem : undefined;
  const field = bodyError(problem);
  if (field) {
    const block = blockIndexOf(field.field, "/body");
    switch (field.code) {
      case "page.image_type":
        return { message: t("pageEdit.problem.imageType"), code: field.code, block };
      case "page.body_invalid":
        return block === undefined
          ? { message: t("pageEdit.problem.tooLarge"), code: field.code }
          : { message: t("pageEdit.problem.bodyInvalid"), code: field.code, block };
      default:
        return { message: t("pageEdit.problem.failed"), code: field.code, block };
    }
  }
  switch (problem?.code) {
    case "file.not_found":
      return { message: t("pageEdit.problem.fileNotFound") };
    case "course.archived":
      return { message: t("pageEdit.problem.archived") };
    case "page.not_found":
      return { message: t("pageEdit.problem.pageGone") };
    case "permission.denied":
      return { message: t("pageEdit.problem.denied") };
    default:
      return { message: t("pageEdit.problem.failed"), code: problem?.code };
  }
}

/** Why an image upload failed, as the sentence the image panel shows. */
export function uploadMessage(error: unknown, t: Strings["t"]): string {
  if (!(error instanceof ApiError)) return t("pageEdit.upload.unreachable");
  switch (error.problem?.code) {
    case "file.too_large":
      return t("pageEdit.upload.tooLarge");
    case "entitlement.limit_reached":
      return t("pageEdit.upload.storageFull");
    case "storage.not_configured":
      return t("pageEdit.upload.noStorage");
    case "course.archived":
      return t("pageEdit.problem.archived");
    default:
      return t("pageEdit.upload.failed");
  }
}
