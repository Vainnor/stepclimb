// SPDX-License-Identifier: AGPL-3.0-or-later
// The `tv()` every recipe uses: tailwind-variants with its class merger told about the token
// theme's own names. Without them the merger reads `text-caption` and `text-read` as colors and
// `font-heading` as a font family, and drops one of two classes that do not conflict
// (`text-caption text-fg-sub`, `font-ui font-heading`). tv.test.ts checks the names against the
// token build.
import { createTV, type TWMergeConfig } from "tailwind-variants/config";

export const twMergeConfig = {
  extend: {
    theme: {
      text: ["caption", "read"],
      "font-weight": ["body", "control", "heading"],
      font: ["ui", "read", "mono"],
      leading: ["read"],
    },
  },
} satisfies TWMergeConfig;

export const tv = createTV({ twMergeConfig });
export type { VariantProps } from "tailwind-variants";
