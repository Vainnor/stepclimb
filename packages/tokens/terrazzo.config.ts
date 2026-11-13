// SPDX-License-Identifier: AGPL-3.0-or-later
// `tz build` reads this file; the settings are in src/config.ts.
import { defineConfig } from "@terrazzo/cli";
import { tokensConfig } from "./src/config";

export default defineConfig(tokensConfig("./src/tokens/resolver.json"));
