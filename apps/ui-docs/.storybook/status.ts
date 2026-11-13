// SPDX-License-Identifier: AGPL-3.0-or-later
// Component status (docs/specs/m0/13-base-ui-components.md B28). Each component's stories carry
// exactly one status tag in their meta (`tags: ["experimental"]`); an experimental component also
// lists the definition-of-done rows it does not meet yet in `parameters.status.missing`.
// Storybook shows the status in the sidebar (manager.ts) and on the component's docs page
// (preview.tsx); `status.test.ts` fails the build when a component has none.

export const statuses = ["experimental", "stable", "deprecated"] as const;
export type Status = (typeof statuses)[number];

/** The rows of the plan's definition of done for a component (UI tab). */
export const definitionOfDone = [
  "States",
  "Themes",
  "Accessibility",
  "Responsive and touch",
  "Tests",
  "Docs",
] as const;

/** The status tags among `tags`, in the order of `statuses`. */
export function statusTags(tags: readonly string[]): Status[] {
  return statuses.filter((s) => tags.includes(s));
}

/** One status, or undefined when there is none or more than one. */
export function statusOf(tags: readonly string[]): Status | undefined {
  const found = statusTags(tags);
  return found.length === 1 ? found[0] : undefined;
}

/** What the check reads of Storybook's index.json. */
export interface IndexEntry {
  type: string;
  title: string;
  tags?: string[];
}

/**
 * Problems with the components' status in a Storybook index: every story must carry exactly one
 * status tag. Docs-only pages (the Tokens page) are not components and are skipped.
 */
export function indexStatusProblems(entries: Record<string, IndexEntry>): string[] {
  const problems = new Set<string>();
  for (const entry of Object.values(entries)) {
    if (entry.type !== "story") continue;
    const found = statusTags(entry.tags ?? []);
    if (found.length === 0) {
      problems.add(`${entry.title}: no status tag (one of ${statuses.join(", ")})`);
    } else if (found.length > 1) {
      problems.add(`${entry.title}: more than one status tag (${found.join(", ")})`);
    }
  }
  return [...problems].sort();
}

/** What the check reads of a stories file's default export. */
export interface StoriesMeta {
  title?: string;
  tags?: string[];
  parameters?: { status?: { missing?: unknown } };
}

/**
 * Problems with a component's status parameters: an experimental component names at least one
 * missing row, each a row of the definition of done; a stable or deprecated one names none.
 */
export function metaStatusProblems(file: string, meta: StoriesMeta): string[] {
  const name = meta.title ?? file;
  const status = statusOf(meta.tags ?? []);
  const missing = meta.parameters?.status?.missing;
  if (status === undefined) return [`${name}: needs exactly one status tag`];
  if (status !== "experimental") {
    return missing === undefined ? [] : [`${name}: is ${status} but lists missing rows`];
  }
  if (!Array.isArray(missing) || missing.length === 0) {
    return [`${name}: is experimental but lists no missing rows in parameters.status.missing`];
  }
  const rows: readonly string[] = definitionOfDone;
  return missing
    .filter((row) => typeof row !== "string" || !rows.includes(row))
    .map((row) => `${name}: "${String(row)}" is not a row of the definition of done`);
}
