// SPDX-License-Identifier: AGPL-3.0-or-later
// Component status (docs/specs/m0/13-base-ui-components.md B28, T11): the check of Storybook's
// index, and of the status parameters every stories file declares. `pnpm test` writes the index
// with `storybook index` first.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  type IndexEntry,
  indexStatusProblems,
  metaStatusProblems,
  type StoriesMeta,
} from "../.storybook/status";

const story = (title: string, tags: string[]): IndexEntry => ({ type: "story", title, tags });

describe("the index check", () => {
  it("fails a component without a status", () => {
    const problems = indexStatusProblems({
      "a--one": story("Primitives/A", ["dev", "autodocs"]),
      "b--one": story("Primitives/B", ["dev", "experimental"]),
    });
    expect(problems).toEqual([
      "Primitives/A: no status tag (one of experimental, stable, deprecated)",
    ]);
  });

  it("fails a component with two statuses", () => {
    expect(indexStatusProblems({ "a--one": story("A", ["stable", "deprecated"]) })).toEqual([
      "A: more than one status tag (stable, deprecated)",
    ]);
  });

  it("skips docs-only pages", () => {
    const tokens = { type: "docs", title: "Foundations/Tokens", tags: ["unattached-mdx"] };
    expect(indexStatusProblems({ "foundations-tokens--docs": tokens })).toEqual([]);
  });

  it("passes the stories Storybook indexes", () => {
    const index = JSON.parse(
      readFileSync(new URL("../storybook-static/index.json", import.meta.url), "utf8"),
    ) as { entries: Record<string, IndexEntry> };
    const stories = Object.values(index.entries).filter((e) => e.type === "story");
    expect(stories.length).toBeGreaterThan(0);
    expect(indexStatusProblems(index.entries)).toEqual([]);
  });
});

describe("the status parameters", () => {
  it("fails an experimental component that lists no missing rows", () => {
    expect(metaStatusProblems("a", { title: "A", tags: ["experimental"] })).toEqual([
      "A: is experimental but lists no missing rows in parameters.status.missing",
    ]);
  });

  it("fails a missing row that is not a row of the definition of done", () => {
    const meta = { title: "A", tags: ["experimental"], parameters: { status: { missing: ["X"] } } };
    expect(metaStatusProblems("a", meta)).toEqual([
      'A: "X" is not a row of the definition of done',
    ]);
  });

  it("fails a stable component that lists missing rows", () => {
    const meta = { title: "A", tags: ["stable"], parameters: { status: { missing: ["Docs"] } } };
    expect(metaStatusProblems("a", meta)).toEqual(["A: is stable but lists missing rows"]);
  });

  it("passes every stories file in Stepclimb UI", () => {
    const files = import.meta.glob<{ default: StoriesMeta }>(
      "../../../packages/ui/src/**/*.stories.tsx",
      { eager: true },
    );
    const entries = Object.entries(files);
    expect(entries.length).toBeGreaterThan(0);
    expect(entries.flatMap(([file, mod]) => metaStatusProblems(file, mod.default))).toEqual([]);
  });
});
