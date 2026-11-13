// SPDX-License-Identifier: AGPL-3.0-or-later
// Every story renders inside the brand, theme and density chosen in the toolbar, at the 1280 or
// 375 px viewport, with Stepclimb UI's strings (docs/specs/m0/13-base-ui-components.md B22, B23).
import "@fontsource/atkinson-hyperlegible-next/400.css";
import "@fontsource/atkinson-hyperlegible-next/500.css";
import "@fontsource/atkinson-hyperlegible-next/700.css";
import "@fontsource/atkinson-hyperlegible-mono/400.css";
import "@fontsource/source-serif-4/400.css";
import "@fontsource/source-serif-4/400-italic.css";
import "@fontsource/source-serif-4/700.css";
import "@fontsource/roboto/400.css";
import "@fontsource/roboto/500.css";
import "@stepclimb/ui/styles.css";
import {
  type Density,
  densities,
  StringsProvider,
  type Theme,
  ThemeProvider,
  themes,
  uiMessages,
} from "@stepclimb/ui";
import {
  type Brand,
  brandAttribute,
  brands,
  densityTitles,
  themeTitles,
} from "@stepclimb/ui/storybook";
import {
  Controls,
  Description,
  Primary,
  Stories,
  Subtitle,
  Title,
  useOf,
} from "@storybook/addon-docs/blocks";
import type { Decorator, Preview } from "@storybook/react-vite";
import { statusOf } from "./status";

const catalogs = [uiMessages];

const withStrings: Decorator = (Story) => (
  <StringsProvider catalogs={catalogs} isDevelopment>
    <Story />
  </StringsProvider>
);

/** A story that draws its own themes (a matrix) sets `parameters.themeWrapper: false`. */
const withTheme: Decorator = (Story, context) => {
  if (context.parameters.themeWrapper === false) return <Story />;
  // A story fills the canvas on its own; on a docs page each one is only as tall as its content.
  const fill = context.viewMode === "docs" ? "" : "min-h-screen";
  return (
    <ThemeProvider
      theme={context.globals.theme as Theme}
      density={context.globals.density as Density}
      brand={brandAttribute(context.globals.brand as Brand)}
    >
      <div className={`${fill} bg-canvas p-6 font-ui text-fg-default`}>
        <Story />
      </div>
    </ThemeProvider>
  );
};

/** The component's status above the generated docs page. */
function StatusNote() {
  const { preparedMeta } = useOf("meta", ["meta"]);
  const status = statusOf(preparedMeta.tags);
  const missing: unknown = preparedMeta.parameters.status?.missing;
  if (!status) return null;
  const rows = Array.isArray(missing) && missing.length > 0 ? missing.join(", ") : null;
  return (
    <p>
      <strong>Status: {status}.</strong>
      {status === "experimental" && rows ? ` Not met yet: ${rows}.` : null}
    </p>
  );
}

function DocsPage() {
  return (
    <>
      <Title />
      <StatusNote />
      <Subtitle />
      <Description />
      <Primary />
      <Controls />
      <Stories />
    </>
  );
}

const preview: Preview = {
  tags: ["autodocs"],
  decorators: [withTheme, withStrings],
  parameters: {
    layout: "fullscreen",
    docs: { page: DocsPage },
    viewport: {
      options: {
        desktop: { name: "1280 px", styles: { width: "1280px", height: "800px" }, type: "desktop" },
        phone: { name: "375 px", styles: { width: "375px", height: "812px" }, type: "mobile" },
      },
    },
  },
  globalTypes: {
    brand: {
      description: "Organization brand",
      toolbar: {
        title: "Brand",
        items: brands.map((b) => ({ value: b.value, title: b.title })),
        dynamicTitle: true,
      },
    },
    theme: {
      description: "Theme",
      toolbar: {
        title: "Theme",
        items: themes.map((t) => ({ value: t, title: themeTitles[t] })),
        dynamicTitle: true,
      },
    },
    density: {
      description: "Density",
      toolbar: {
        title: "Density",
        items: densities.map((d) => ({ value: d, title: densityTitles[d] })),
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: {
    brand: "default",
    theme: "light",
    density: "comfortable",
    viewport: { value: "desktop", isRotated: false },
  },
};

export default preview;
