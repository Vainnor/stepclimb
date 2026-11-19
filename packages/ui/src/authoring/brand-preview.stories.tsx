// SPDX-License-Identifier: AGPL-3.0-or-later
import { generate } from "@stepclimb/tokens/generate";
import { contrastReport } from "@stepclimb/tokens/report";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { BrandPreview } from "./brand-preview";

/** A sample brand color from its 24-bit RGB number: a brand color is data, not a style. */
const hex = (n: number) => `#${n.toString(16).padStart(6, "0")}`;

const appearances = ["light", "dark", "high-contrast"] as const;

function draft(primary: string) {
  const themes = generate({ primary });
  return { themes, reports: appearances.map((a) => contrastReport(themes[a])) };
}

/** The default brand with light `fg-soft` moved toward the canvas: a brand the report refuses. */
function refused() {
  const themes = generate({ primary: hex(0x007373) });
  const canvas = themes.light.colors["bg-canvas"];
  themes.light.colors["fg-soft"] = { ...canvas, l: canvas.l - 0.12 };
  return { themes, reports: appearances.map((a) => contrastReport(themes[a])) };
}

const meta = {
  title: "Authoring/BrandPreview",
  component: BrandPreview,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Responsive and touch"] },
    docs: {
      description: {
        component: `A draft brand before it is saved: a sample screen (top bar, a course row, a button, a callout and text) in the draft's light, dark and high-contrast themes side by side, then the theme generator's contrast report. The samples are pictures, not controls; the report says in words whether the colors can be saved and lists every measured pair, failing pairs first.

**Usage.** \`<BrandPreview themes={generate(inputs)} reports={appearances.map((a) => contrastReport(themes[a]))} />\`, with \`generate\` from \`@stepclimb/tokens/generate\` and \`contrastReport\` from \`@stepclimb/tokens/report\`; pass the server's report instead when it refused the brand.

**Do** show it beside the branding fields, so a change is seen before it is saved.

**Don't** let a brand whose report fails be saved: the server refuses it too.`,
      },
    },
  },
  args: draft(hex(0x007373)),
} satisfies Meta<typeof BrandPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const DefaultBrand: Story = {};

export const VzdcBrand: Story = {
  name: "vZDC brand, with the brand-near-danger warning",
  args: draft(hex(0x500e0e)),
};

export const PaleYellow: Story = { name: "Pale yellow brand", args: draft(hex(0xfff4b0)) };

export const NearBlackNavy: Story = { name: "Near-black navy brand", args: draft(hex(0x0a1028)) };

export const Refused: Story = {
  name: "Refused: a failing contrast pair",
  args: refused(),
};
