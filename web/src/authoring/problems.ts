// SPDX-License-Identifier: AGPL-3.0-or-later
// What the course authoring screen says when a write fails (docs/specs/m1/03-modules-and-items.md
// API and its I1 decisions; 04-unlock-and-completion-rules.md API and its I1 decisions;
// 07-pages-and-the-editor.md B1): one message per problem `code`, each saying what happened and
// what to do, and for a refused field the member it names, so the form can show the message
// there. An unknown code gets a general message and the code itself, so the person can report it.
import type { Strings } from "@stepclimb/ui";
import { ApiError, isUnreachable } from "../api";

export interface AuthoringProblem {
  /** What happened and what to do. */
  message: string;
  /** The server's code, shown in mono, when the screen has no message of its own for it. */
  code?: string;
  /** The request member a 422 names (`title`, `url`, `unlock_at`...), when it names one. */
  field?: string;
}

/** The message for a rule the member `field` broke, from a 422's first `errors` entry. */
function fieldMessage(code: string, field: string | undefined, t: Strings["t"]): string | undefined {
  if (code === "text.contains_nul") {
    if (field === "title") return t("authoring.problem.titleInvalid");
    if (field === "url") return t("authoring.problem.urlCharacter");
    return undefined;
  }
  switch (code) {
    case "name.empty":
      return t("authoring.problem.titleEmpty");
    case "name.too_long":
      return t("authoring.problem.titleTooLong");
    case "item.url_invalid":
      return t("authoring.problem.urlInvalid");
    case "item.url_required":
      return t("authoring.problem.urlRequired");
    case "module.date_out_of_range":
      return t("authoring.problem.dateOutOfRange");
    case "module.prerequisites_too_many":
      return t("authoring.problem.prerequisitesTooMany");
    case "module.prerequisite_self":
    case "module.prerequisite_duplicate":
      return t("authoring.problem.prerequisitesInvalid");
    case "requirement.invalid":
    case "requirement.unavailable":
      return t("authoring.problem.requirementInvalid");
    case "page.slug_invalid":
      return t("authoring.problem.pageSlugInvalid");
    default:
      return undefined;
  }
}

/** The message for a problem `code` the screen explains, or undefined. */
function codeMessage(code: string | undefined, t: Strings["t"]): string | undefined {
  switch (code) {
    case "course.archived":
      return t("authoring.archived");
    case "course.not_found":
      return t("authoring.problem.courseGone");
    case "module.not_found":
      return t("authoring.problem.moduleGone");
    case "item.not_found":
      return t("authoring.problem.itemGone");
    case "module.not_empty":
      return t("authoring.problem.moduleNotEmpty");
    case "item.has_progress":
      return t("authoring.problem.itemHasProgress");
    case "module.prerequisite_cycle":
      return t("authoring.problem.prerequisiteCycle");
    case "module.limit_reached":
      return t("authoring.problem.moduleLimit");
    case "item.limit_reached":
      return t("authoring.problem.itemLimit");
    case "page.slug_taken":
      return t("authoring.problem.pageSlugTaken");
    case "permission.denied":
      return t("authoring.problem.denied");
    default:
      return undefined;
  }
}

/**
 * What to tell the person about `error`, a failed write of the authoring screen. `drawn` names the
 * request members the form has a field for; a 422 on any other member (`/kind`, `/position`, a
 * nested pointer) is a problem of the whole form, shown above it, so no refusal goes unshown.
 */
export function authoringProblem(
  error: unknown,
  t: Strings["t"],
  drawn: readonly string[] = [],
): AuthoringProblem {
  if (isUnreachable(error)) return { message: t("authoring.problem.unreachable") };
  if (!(error instanceof ApiError)) return { message: t("authoring.problem.failed") };
  const problem = error.problem;
  const first = problem?.errors?.[0];
  if (first !== undefined) {
    const member = first.field.split("/")[1];
    const message = fieldMessage(first.code, member, t);
    const field = member !== undefined && drawn.includes(member) ? member : undefined;
    if (message !== undefined) return { message, field };
    return { message: t("authoring.problem.unknown"), code: first.code, field };
  }
  const known = codeMessage(problem?.code, t);
  if (known !== undefined) return { message: known };
  return { message: t("authoring.problem.unknown"), code: problem?.code ?? `HTTP ${error.status}` };
}

/** True when a write named a module or item that is gone, so the board on screen is stale. */
export function isGone(error: unknown): boolean {
  if (!(error instanceof ApiError)) return false;
  const code = error.problem?.code;
  return code === "module.not_found" || code === "item.not_found";
}
