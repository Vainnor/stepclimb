// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { createToastQueue, type ToastMessage, ToastRegion } from "./toast";

const meta = {
  title: "Overlays/Toast",
  component: ToastRegion,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A short notice of what just happened: at the bottom inline end of the screen from 768 px, across the top of a phone. Screen readers announce it as it appears; it never takes focus, and F6 reaches the region of notices. Hovering a notice or focusing inside it stops every timer until the pointer or focus leaves (WCAG 2.2.1). A notice that is not an error goes after 6 seconds (never under 5); an error stays until it is dismissed. At most three show at once, the newest first; the rest wait their turn. Nothing moves, so there is no motion to reduce.

**Usage.** Make one queue for the app and render its region once: \`export const toasts = createToastQueue();\` and \`<ToastRegion queue={toasts} />\` near the root. Then \`toasts.add({ title: t("course.saved"), tone: "success" })\`. \`tone\` is \`neutral\` (the default), \`success\` or \`error\`; \`description\` says what to do next.

**Do** keep it to one sentence, and use it for the result of something the person just did.

**Don't** make a toast the only place an error shows: the field, form or page that failed says so too, where the person will look for it. Don't put actions in it (it may be gone before they are reached), and don't use it for something the person must decide (that is an \`AlertDialog\`).`,
      },
    },
  },
  args: { queue: createToastQueue() },
  argTypes: { queue: { control: false } },
} satisfies Meta<typeof ToastRegion>;

export default meta;
type Story = StoryObj<typeof meta>;

// Stories hold their toasts for an hour, so a screenshot never races the timer.
const HOUR_MS = 60 * 60 * 1000;

const saved: ToastMessage = { title: "Course saved", tone: "success" };
const published: ToastMessage = {
  title: "Module published",
  description: "Learners enrolled in the course see it now.",
};
const failed: ToastMessage = {
  title: "Could not sign out",
  description: "Check your connection and sign out again.",
  tone: "error",
};

/**
 * A page with a region, its queue already holding `initial`, and buttons that add more. It fills
 * the screen, so the story's screenshot reaches the edge the toasts sit at.
 */
function Page({ initial, fill = true }: { initial: readonly ToastMessage[]; fill?: boolean }) {
  const [queue] = useState(() => {
    const made = createToastQueue();
    for (const message of initial) made.add(message, { timeout: HOUR_MS });
    return made;
  });
  return (
    <div
      className={`flex flex-wrap content-start gap-2 ${fill ? "min-h-[calc(100dvh-3rem)]" : ""}`}
    >
      <Button onPress={() => queue.add(saved)}>Save course</Button>
      <Button onPress={() => queue.add(failed)}>Sign out</Button>
      <ToastRegion queue={queue} />
    </div>
  );
}

/** A success notice. */
export const Success: Story = { render: () => <Page initial={[saved]} /> };

/** A neutral notice with what it means for the person. */
export const Neutral: Story = {
  render: () => <Page initial={[published]} />,
};

/** An error, which stays until it is dismissed. */
export const ErrorToast: Story = {
  name: "Error",
  render: () => <Page initial={[failed]} />,
};

/** Three at once, the newest first. */
export const Stacked: Story = {
  render: () => <Page initial={[failed, published, saved]} />,
};

export const LongContent: Story = {
  render: () => (
    <Page
      initial={[
        {
          title: "Ground control check and Local control check were assigned to 24 learners",
          description:
            "Each learner has until 30 November to finish both. Their managers see the due dates on their team page.",
          tone: "success",
        },
      ]}
    />
  ),
};

/** The buttons that add toasts in every brand, theme and density; the toasts themselves draw in the
 * page's appearance, at the screen's edge. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Page initial={[]} fill={false} />
    </ThemeMatrix>
  ),
};
