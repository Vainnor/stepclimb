// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { NextUp, type NextUpStatus } from "./next-up";

const next: NextUpStatus = {
  kind: "next",
  itemTitle: "Taxi instructions",
  moduleTitle: "Ground control",
  href: "#taxi-instructions",
  started: false,
};

const meta = {
  title: "Learning/NextUp",
  component: NextUp,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The one thing to do next in a course, with the view's one primary action, "Continue". The server works out what is next from the same rules that lock content, so the panel shows its answer: the next item and its module, or, when nothing is left to do yet, when the course opens further, or that it is complete. Dates read in the user's time zone.

**Usage.** On the course home, under the course header: \`<NextUp status={...} progress={{ completed, required }} timeZone={me.tz} />\`. On the dashboard, for the first course, add \`courseTitle\`. Map the API's \`next_up\` to \`{ kind: "next", itemTitle, moduleTitle, href: next_up.link, started: next_up.progress !== null }\`, \`waiting_until\` to \`{ kind: "waiting", until }\` and \`completed_at\` to \`{ kind: "complete", completedAt }\`.

**Do** keep it the only primary action on the page.

**Don't** pick the next item in the browser, or add statistics to the panel.`,
      },
    },
  },
  args: { status: next, timeZone: "America/New_York", progress: { completed: 3, required: 7 } },
} satisfies Meta<typeof NextUp>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

/** The learner has opened the item but not finished it. */
export const Started: Story = { args: { status: { ...next, started: true } } };

/** On the dashboard, the panel names the course. */
export const Dashboard: Story = { args: { courseTitle: "S1 Ground theory" } };

/** Everything the learner can reach is done; the next module opens on a date. */
export const Waiting: Story = {
  args: { status: { kind: "waiting", until: new Date("2026-03-03T15:00:00Z") } },
};

export const Completed: Story = {
  args: {
    status: { kind: "complete", completedAt: new Date("2026-03-03T15:00:00Z") },
    progress: { completed: 7, required: 7 },
  },
};

export const LongContent: Story = {
  args: {
    courseTitle: "S3 Approach control: radar vectoring, sequencing and the arrival flow",
    status: {
      ...next,
      itemTitle: "Simultaneous independent approaches to parallel runways in instrument conditions",
      moduleTitle: "Coordination with adjacent facilities during a ground stop",
    },
  },
};

const continueLink = (canvasElement: HTMLElement) =>
  within(canvasElement).findByRole("link", { name: /^Continue/ });

/**
 * Hovers `link` until React Aria marks it hovered: a hover sent as the story first renders can
 * arrive before React Aria listens, and is then lost. It waits for the fonts and then for the page
 * to settle first: after a layout change, Chromium sends a pointer event for wherever its own
 * cursor rests, which ends React Aria's hover. A screen without hover (a phone) shows the link as
 * it is.
 */
async function hover(link: HTMLElement) {
  if (window.matchMedia("(hover: none)").matches) return;
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 300));
  await waitFor(async () => {
    await userEvent.unhover(link);
    await userEvent.hover(link);
    await expect(link).toHaveAttribute("data-hovered");
  });
}

/** The pointer over "Continue". */
export const Hover: Story = {
  play: async ({ canvasElement }) => {
    await hover(await continueLink(canvasElement));
  },
};

/** Keyboard focus on "Continue". */
export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    const link = await continueLink(canvasElement);
    await userEvent.tab();
    await waitFor(() => expect(link).toHaveAttribute("data-focus-visible"));
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <NextUp {...args} courseTitle="S1 Ground theory" />
      <NextUp {...args} status={{ kind: "waiting", until: new Date("2026-03-03T15:00:00Z") }} />
    </ThemeMatrix>
  ),
};
