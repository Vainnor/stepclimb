// SPDX-License-Identifier: AGPL-3.0-or-later
// The changeset check (bin/check-changesets.mjs; spec 13 B31, Q6). Run by `just web-lint`.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { missingChangesets } from "./check-changesets.mjs";

const packages = [
  { name: "stepclimb", dir: "" },
  { name: "@stepclimb/ui", dir: "packages/ui" },
  { name: "@stepclimb/tokens", dir: "packages/tokens" },
  { name: "web", dir: "web" },
];
const ignore = ["web"];
const changeset = (id, ...names) => ({ id, releases: names.map((name) => ({ name })) });

function missing(changedFiles, ...changesets) {
  return missingChangesets({ changedFiles, packages, ignore, status: { changesets } });
}

describe("the changeset check", () => {
  it("fails a ui change that carries only a tokens changeset", () => {
    const files = ["packages/ui/src/index.ts", ".changeset/tokens.md"];
    assert.deepEqual(missing(files, changeset("tokens", "@stepclimb/tokens")), ["@stepclimb/ui"]);
  });
  it("fails a change with no changeset", () => {
    assert.deepEqual(missing(["packages/tokens/src/tokens/color.json"]), ["@stepclimb/tokens"]);
  });
  it("passes when a changeset on the branch names every changed package", () => {
    const files = ["packages/ui/src/index.ts", "packages/tokens/x.json", ".changeset/both.md"];
    const both = changeset("both", "@stepclimb/ui", "@stepclimb/tokens");
    assert.deepEqual(missing(files, both), []);
  });
  it("does not count a pending changeset an earlier PR added", () => {
    assert.deepEqual(missing(["packages/ui/src/index.ts"], changeset("old", "@stepclimb/ui")), [
      "@stepclimb/ui",
    ]);
  });
  it("does not count an empty changeset", () => {
    const files = ["packages/ui/src/index.ts", ".changeset/empty.md"];
    assert.deepEqual(missing(files, changeset("empty")), ["@stepclimb/ui"]);
  });
  it("passes changes to ignored packages and to the repository root", () => {
    assert.deepEqual(missing(["web/src/main.tsx", "justfile", "packages/uikit.md"]), []);
  });
});
