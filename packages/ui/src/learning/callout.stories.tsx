// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Callout, calloutKinds } from "./callout";

const text: Record<(typeof calloutKinds)[number], [string, string]> = {
  info: ["Readbacks", "Read back every clearance in full before you taxi."],
  tip: ["Practice first", "The drill on the next page uses the same phraseology."],
  warning: ["Check the frequency", "Two airports in this module share a ground frequency."],
  danger: ["Runway incursion risk", "Never clear an aircraft across an active runway unasked."],
};

const meta = {
  title: "Learning/Callout",
  component: Callout,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A note set apart from the text around it: info, tip, warning or danger. It is an icon on a tinted surface, never a colored stripe at its side; each kind has its own icon, which a screen reader reads as the kind ("Warning").

**Usage.** \`<Callout kind="warning" title="Check the frequency">…</Callout>\`, in a page or an announcement.

**Do** keep it to a sentence or two, and pick the kind by what the reader risks: danger for harm, warning for a mistake.

**Don't** use it for form errors or feedback on answers, or stack several in a row.`,
      },
    },
  },
  args: { kind: "info", title: text.info[0], children: text.info[1] },
} satisfies Meta<typeof Callout>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { className: "max-w-xl" } };

export const AllKinds: Story = {
  render: () => (
    <div className="flex max-w-xl flex-col gap-4">
      {calloutKinds.map((kind) => (
        <Callout key={kind} kind={kind} title={text[kind][0]}>
          {text[kind][1]}
        </Callout>
      ))}
    </div>
  ),
};

export const WithoutTitle: Story = {
  render: () => (
    <div className="flex max-w-xl flex-col gap-4">
      {calloutKinds.map((kind) => (
        <Callout key={kind} kind={kind}>
          {text[kind][1]}
        </Callout>
      ))}
    </div>
  ),
};

export const LongContent: Story = {
  args: {
    className: "max-w-xs",
    kind: "warning",
    title: "Coordination with adjacent facilities during a ground stop or a flow program",
    children:
      "When a ground stop is in effect, coordinate every release with the overlying facility before the aircraft taxis, and record who approved it, the time and any conditions they set.",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      {calloutKinds.map((kind) => (
        <Callout key={kind} kind={kind} title={text[kind][0]}>
          {text[kind][1]}
        </Callout>
      ))}
    </ThemeMatrix>
  ),
};
