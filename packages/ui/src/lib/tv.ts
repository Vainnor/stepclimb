// SPDX-License-Identifier: AGPL-3.0-or-later
// The `tv()` every recipe uses: tailwind-variants with its class merger told about the token
// theme's own names. Without them the merger reads `text-caption` and `text-read` as colors and
// `font-heading` as a font family, and drops one of two classes that do not conflict
// (`text-caption text-fg-sub`, `font-ui font-heading`); and it keeps both of two classes that do
// conflict when one is a token name it does not know (`rounded-sm rounded-pill`,
// `duration-fast duration-slow`). tv.test.ts checks every name against the token build.
import { createTV, type TWMergeConfig } from "tailwind-variants/config";

export const twMergeConfig = {
  extend: {
    theme: {
      text: ["caption", "read"],
      "font-weight": ["body", "control", "heading"],
      font: ["ui", "read", "mono"],
      leading: ["read"],
      radius: ["pill"],
      shadow: ["float"],
      ease: ["out-expo"],
    },
    classGroups: {
      // The motion tokens are utilities of their own (`@utility duration-fast`), not a theme scale.
      duration: [{ duration: ["fast", "base", "slow"] }],
    },
  },
} satisfies TWMergeConfig;

export const tv = createTV({ twMergeConfig });
export type { VariantProps } from "tailwind-variants";
