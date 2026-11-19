// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { AlertDialog } from "./alert-dialog";
import { Button } from "./button";
import { DialogTrigger } from "./dialog";

const meta = {
  title: "Overlays/AlertDialog",
  component: AlertDialog,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Asks before an action that matters. Screen readers announce it as an alert dialog. It has no close button and a press outside does nothing: it ends with Cancel, Escape or the action. Focus starts on Cancel, so pressing Enter on arrival never runs the action.

**Usage.** \`<DialogTrigger><Button intent="danger">Delete</Button><AlertDialog title="Delete Ground control?" confirmLabel="Delete module" intent="danger" onConfirm={remove}>…</AlertDialog></DialogTrigger>\`. \`isOpen\` and \`onOpenChange\` control it.

**Do** ask a question in the title, say in the body what happens and whether it can be undone, and name the action on the confirm button. Use \`intent="danger"\` when the action destroys something.

**Don't** ask for something that is easy to undo; do it and offer an undo instead.`,
      },
    },
  },
  args: { title: "", confirmLabel: "", onConfirm: () => {}, children: null },
} satisfies Meta<typeof AlertDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function Delete({
  defaultOpen,
  fill = true,
}: {
  defaultOpen?: boolean;
  /** As tall as the screen, for an overlay to open over; a matrix cell leaves it closed. */
  fill?: boolean;
}) {
  return (
    <div className={fill ? "min-h-[calc(100dvh-3rem)]" : undefined}>
      <DialogTrigger defaultOpen={defaultOpen}>
        <Button intent="danger">Delete module</Button>
        <AlertDialog
          title="Delete Ground control?"
          confirmLabel="Delete module"
          intent="danger"
          onConfirm={() => {}}
        >
          Its four pages and every learner's progress in them are deleted. This cannot be undone.
        </AlertDialog>
      </DialogTrigger>
    </div>
  );
}

/** A destructive action: the confirm button is in the danger intent. */
export const Danger: Story = { render: () => <Delete defaultOpen /> };

/** An action that is not destructive keeps the primary intent. */
export const Primary: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <AlertDialog
        defaultOpen
        title="Publish Ground control?"
        confirmLabel="Publish module"
        onConfirm={() => {}}
      >
        Learners enrolled in the course see it at once and get a notification.
      </AlertDialog>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Delete fill={false} />
    </ThemeMatrix>
  ),
};
