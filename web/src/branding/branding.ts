// SPDX-License-Identifier: AGPL-3.0-or-later
// The organization's branding through the public API (docs/specs/m0/12-tokens-and-theme-generator.md
// API, B21, B24, B27): the saved inputs and report, the save, and what the screen says about a
// refused save, field by field. The server decides every refusal; the draft the browser builds
// (`draft.ts`) only lets the screen show the report before Save is pressed.
import type { components } from "@stepclimb/api-client";
import type { Strings } from "@stepclimb/ui";
import { queryOptions } from "@tanstack/react-query";
import { ApiError, api, isUnreachable, unwrap } from "../api";

export type Branding = components["schemas"]["Branding"];
export type SavedInputs = components["schemas"]["SavedInputs"];
export type ServerReport = components["schemas"]["ContrastReport"];

export const brandingQuery = queryOptions({
  queryKey: ["admin", "branding"],
  queryFn: async (): Promise<Branding> => unwrap(await api.GET("/api/v1/admin/branding")),
});

/** Replaces the organization's branding with `inputs`. */
export async function saveBranding(inputs: SavedInputs): Promise<Branding> {
  return unwrap(await api.PUT("/api/v1/admin/branding", { body: inputs }));
}

/** The inputs a save sends, by the name each field has in the form and the API. */
export const brandingFields = [
  "primary",
  "ui_font",
  "reading_font",
  "corner_style",
  "default_appearance",
] as const;
export type BrandingField = (typeof brandingFields)[number];

/** What the screen shows for a failed save: messages at fields, a report, or one message. */
export interface BrandingRefusal {
  fields: Partial<Record<BrandingField, string>>;
  /** The server's contrast report, when it refused the colors. */
  report?: ServerReport[];
  /** A refusal no field owns: what happened and what to do, and an unknown code. */
  message?: string;
  code?: string;
}

function isBrandingField(name: string): name is BrandingField {
  return (brandingFields as readonly string[]).includes(name);
}

/** The message for a rule the server says a field broke (`branding.*` in `errors[]`). */
function fieldMessage(field: BrandingField, code: string, t: Strings["t"]): string {
  if (field === "primary" && code === "branding.invalid_color") {
    return t("branding.problem.invalidColor");
  }
  if (field === "primary" && code === "branding.too_long") return t("branding.problem.tooLong");
  return t("branding.problem.notAChoice");
}

/** The report a `branding.contrast_failed` answer carries, when it is well formed. */
function reportOf(problem: unknown): ServerReport[] | undefined {
  if (typeof problem !== "object" || problem === null || !("report" in problem)) return undefined;
  const { report } = problem;
  if (!Array.isArray(report)) return undefined;
  const wellFormed = report.every(
    (entry) =>
      typeof entry === "object" &&
      entry !== null &&
      typeof entry.appearance === "string" &&
      typeof entry.pass === "boolean" &&
      Array.isArray(entry.pairs) &&
      Array.isArray(entry.warnings),
  );
  return wellFormed ? (report as ServerReport[]) : undefined;
}

function invalidInput(error: ApiError, t: Strings["t"]): BrandingRefusal {
  const fields: BrandingRefusal["fields"] = {};
  for (const entry of error.problem?.errors ?? []) {
    const name = entry.field.replace(/^\//, "");
    if (isBrandingField(name)) fields[name] = fieldMessage(name, entry.code, t);
  }
  if (Object.keys(fields).length > 0) return { fields };
  return { fields, message: t("branding.problem.unknown"), code: error.problem?.code };
}

/** What to show for `error`, a failed save. */
export function brandingRefusal(error: unknown, t: Strings["t"]): BrandingRefusal {
  if (isUnreachable(error)) return { fields: {}, message: t("branding.problem.unreachable") };
  if (!(error instanceof ApiError)) return { fields: {}, message: t("branding.problem.failed") };
  const code = error.problem?.code;
  if (code === "branding.contrast_failed") {
    return {
      fields: { primary: t("branding.problem.contrast") },
      report: reportOf(error.problem),
    };
  }
  if (code === "branding.invalid_input") return invalidInput(error, t);
  if (code === "entitlement.feature_unavailable") {
    return { fields: {}, message: t("branding.problem.notInPlan") };
  }
  if (code === "permission.denied") return { fields: {}, message: t("branding.problem.forbidden") };
  return {
    fields: {},
    message: t("branding.problem.unknown"),
    code: code ?? `HTTP ${error.status}`,
  };
}

/**
 * Points the page's theme link at the saved stylesheet, so the page shows the new brand without a
 * reload. The server's `/theme.css?v=<hash>` answers that stylesheet (m0/11 B20).
 */
export function linkSavedStylesheet(cssHash: string): void {
  const link = document.querySelector<HTMLLinkElement>(
    'link[rel="stylesheet"][href^="/theme.css"]',
  );
  if (link) link.href = `/theme.css?v=${encodeURIComponent(cssHash)}`;
}
