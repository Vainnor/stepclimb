// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs the package's Terrazzo build in memory against any resolver, so a test can build a
// modified copy of the token JSON with exactly the settings `tz build` uses.
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { loadTokens } from "@terrazzo/cli";
import { build, defineConfig, Logger, parse } from "@terrazzo/parser";
import { tokensConfig } from "../../src/config";

export interface Built {
  variables: string;
  tailwind: string;
  /** build/docs/tokens.json, the docs tables' data. */
  docs: string;
  /** build/docs/tokens.md. */
  docsMarkdown: string;
}

export const tokensDir = new URL("../../src/tokens/", import.meta.url);

export async function buildTokens(dir: URL = tokensDir): Promise<Built> {
  const logger = new Logger({ level: "error" });
  const config = defineConfig(tokensConfig(new URL("resolver.json", dir).href), {
    cwd: new URL("../../", import.meta.url),
    logger,
  });
  const raw = await loadTokens(config.tokens, { logger });
  if (!raw) throw new Error(`could not load ${dir.href}`);
  const { tokens, resolver, sources } = await parse(raw, { config, logger });
  const { outputFiles } = await build(tokens, { resolver, sources, config, logger });
  const file = (name: string) => {
    const out = outputFiles.find((f) => f.filename === name);
    if (!out) throw new Error(`the build wrote no ${name}`);
    return String(out.contents);
  };
  return {
    variables: file("css/variables.css"),
    tailwind: file("css/tailwind-theme.css"),
    docs: file("docs/tokens.json"),
    docsMarkdown: file("docs/tokens.md"),
  };
}

/**
 * Builds a temporary copy of the token JSON with one file rewritten by `edit`, and removes the
 * copy afterwards.
 */
export async function buildEdited(file: string, edit: (json: string) => string): Promise<Built> {
  const dir = mkdtempSync(join(tmpdir(), "sc-tokens-"));
  try {
    cpSync(tokensDir, dir, { recursive: true });
    const path = join(dir, file);
    const before = readFileSync(path, "utf8");
    const after = edit(before);
    if (after === before) throw new Error(`the edit did not change ${file}`);
    writeFileSync(path, after);
    return await buildTokens(pathToFileURL(`${dir}/`));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
