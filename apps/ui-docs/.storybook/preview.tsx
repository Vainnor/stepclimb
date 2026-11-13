// SPDX-License-Identifier: AGPL-3.0-or-later
// Every story renders inside the brand and appearance chosen in the toolbar.
import "@fontsource/atkinson-hyperlegible-next/400.css";
import "@fontsource/atkinson-hyperlegible-next/500.css";
import "@fontsource/atkinson-hyperlegible-next/700.css";
import "@fontsource/atkinson-hyperlegible-mono/400.css";
import "@fontsource/source-serif-4/400.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@stepclimb/ui/styles.css";
import type { Decorator, Preview } from "@storybook/react-vite";

const withTheme: Decorator = (Story, context) => {
  if (context.parameters.themeWrapper === false) return <Story />;
  const brand = context.globals.brand as string;
  return (
    <div
      data-sc-brand={brand === "default" ? undefined : brand}
      data-sc-theme={context.globals.appearance as string}
      className="min-h-screen bg-canvas p-6 font-ui text-fg-default"
    >
      <Story />
    </div>
  );
};

const preview: Preview = {
  decorators: [withTheme],
  parameters: { layout: "fullscreen" },
  globalTypes: {
    brand: {
      description: "Organization brand",
      toolbar: {
        title: "Brand",
        items: [
          { value: "default", title: "Stepclimb default" },
          { value: "vzdc", title: "vZDC" },
        ],
        dynamicTitle: true,
      },
    },
    appearance: {
      description: "Light or dark",
      toolbar: {
        title: "Appearance",
        items: [
          { value: "light", title: "Light" },
          { value: "dark", title: "Dark" },
        ],
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: { brand: "default", appearance: "light" },
};

export default preview;
