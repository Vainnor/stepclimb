// SPDX-License-Identifier: Apache-2.0
// Generate src/schema.ts from the committed OpenAPI document.
import { readFileSync, writeFileSync } from "node:fs";
import openapiTS, { astToString } from "openapi-typescript";

const specUrl = new URL("../../../openapi/stepclimb.json", import.meta.url);
// `node scripts/generate.mjs [OUT]` writes OUT instead, which `just openapi-diff` compares.
const outUrl = process.argv[2]
  ? new URL(process.argv[2], `file://${process.cwd()}/`)
  : new URL("../src/schema.ts", import.meta.url);

const ast = await openapiTS(JSON.parse(readFileSync(specUrl, "utf8")));
const header = [
  "// SPDX-License-Identifier: Apache-2.0",
  "// Generated from openapi/stepclimb.json by scripts/generate.mjs. Do not edit; run",
  "// `just openapi` instead.",
  "",
].join("\n");
writeFileSync(outUrl, `${header}${astToString(ast)}`);
