// SPDX-License-Identifier: AGPL-3.0-or-later
// Storybook for Stepclimb UI (plan, UI tab -> Library architecture: apps/ui-docs). Stories live
// beside their components in packages/ui; docs-only pages (Tokens) live in src/.
import type { StorybookConfig } from "@storybook/react-vite";
import tailwindcss from "@tailwindcss/vite";

const config: StorybookConfig = {
  stories: ["../src/**/*.mdx", "../../../packages/ui/src/**/*.stories.@(ts|tsx)"],
  addons: ["@storybook/addon-docs"],
  framework: { name: "@storybook/react-vite", options: {} },
  core: { disableTelemetry: true },
  async viteFinal(config) {
    config.plugins = [...(config.plugins ?? []), tailwindcss()];
    return config;
  },
};

export default config;
