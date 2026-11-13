#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Every versioned workspace package this branch changed is named by a changeset this branch adds
// (docs/specs/m0/13-base-ui-components.md B31, Q6; .changeset/README.md). `changeset status` alone
// passes when any changeset exists, so a change to packages/ui carrying only a tokens changeset
// would slip through; this check compares package by package.
//
// Versioned packages are the workspace packages outside `ignore` in .changeset/config.json. A
// package counts as changed when a file under its directory differs from the merge base with the
// base ref, in the working tree, as Changesets itself diffs. A changeset counts when its file is
// among those changes, so a changeset left pending from an earlier PR does not cover this one.
//
// Usage: check-changesets.mjs [BASE_REF]   (default origin/next)
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * The versioned packages that changed but no changed changeset names.
 * @param {object} input
 * @param {string[]} input.changedFiles repository-relative paths changed since the merge base
 * @param {{name: string, dir: string}[]} input.packages workspace packages, `dir` relative to the root
 * @param {string[]} input.ignore package names Changesets does not version
 * @param {{changesets: {id: string, releases: {name: string}[]}[]}} input.status
 *   `changeset status --output` JSON
 */
export function missingChangesets({ changedFiles, packages, ignore, status }) {
  const versioned = packages.filter((p) => p.dir !== "" && !ignore.includes(p.name));
  const inside = (file, dir) => file === dir || file.startsWith(`${dir}/`);
  const changed = versioned.filter((p) => changedFiles.some((f) => inside(f, p.dir)));
  const added = new Set(changedFiles.filter((f) => /^\.changeset\/[^/]+\.md$/.test(f)));
  const named = new Set(
    status.changesets
      .filter((c) => added.has(`.changeset/${c.id}.md`))
      .flatMap((c) => c.releases.map((r) => r.name)),
  );
  return changed.map((p) => p.name).filter((name) => !named.has(name));
}

function git(args) {
  return execFileSync("git", args, { encoding: "utf8" }).trim();
}

function workspacePackages(root) {
  const out = execFileSync("pnpm", ["ls", "-r", "--depth", "-1", "--json"], { encoding: "utf8" });
  return JSON.parse(out).map((p) => ({ name: p.name, dir: relative(root, p.path) }));
}

function changesetStatus(base) {
  const dir = mkdtempSync(join(tmpdir(), "stepclimb-changesets-"));
  try {
    const file = join(dir, "status.json");
    // Exits non-zero when changed packages have no changeset at all; the comparison below then
    // names them, so the exit code is not needed here.
    try {
      execFileSync("pnpm", ["changeset", "status", `--since=${base}`, `--output=${file}`], {
        stdio: ["ignore", "ignore", "inherit"],
      });
    } catch (err) {
      if (typeof err.status !== "number") throw err;
    }
    try {
      return JSON.parse(readFileSync(file, "utf8"));
    } catch (err) {
      if (err.code === "ENOENT") return { changesets: [] };
      throw err;
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function main(base) {
  const root = git(["rev-parse", "--show-toplevel"]);
  const mergeBase = git(["merge-base", base, "HEAD"]);
  const changedFiles = git(["diff", "--name-only", "--no-relative", mergeBase])
    .split("\n")
    .filter(Boolean);
  const config = JSON.parse(readFileSync(join(root, ".changeset/config.json"), "utf8"));
  const missing = missingChangesets({
    changedFiles,
    packages: workspacePackages(root),
    ignore: config.ignore ?? [],
    status: changesetStatus(base),
  });
  if (missing.length > 0) {
    for (const name of missing) {
      console.error(
        `changesets: ${name} changed since ${base} but no changeset on this branch names it; run \`pnpm changeset\` and commit the file`,
      );
    }
    return 1;
  }
  console.log(`changesets: ok (since ${base})`);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main(process.argv[2] ?? "origin/next");
}
