// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { Dialog, DialogBody, DialogFooter, DialogTrigger } from "./dialog";
import { TextField } from "./text-field";

const meta = {
  title: "Overlays/Dialog",
  component: Dialog,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A modal window for a confirmation or a short input: a name, a date, one choice. Focus moves into it and cannot leave until it closes; Escape, the close button or a press on the dimmed, blurred page behind it closes it and returns focus to the button that opened it.

**Usage.** \`<DialogTrigger><Button>Rename</Button><Dialog title="Rename module">{({ close }) => <><DialogBody>…</DialogBody><DialogFooter><Button onPress={close}>Cancel</Button><Button intent="primary">Save name</Button></DialogFooter></>}</Dialog></DialogTrigger>\`. \`isOpen\` and \`onOpenChange\` on \`DialogTrigger\`, or on \`Dialog\` alone, control it. \`isDismissable={false}\` keeps a press outside from closing it.

**Do** give it a title that says what it is for, and name the action that ends it ("Save name", not "OK").

**Don't** use a dialog for work: editing a page, grading or anything longer than a few fields belongs on a page or in a \`Sheet\`.`,
      },
    },
  },
  args: { title: "Rename module", children: null },
} satisfies Meta<typeof Dialog>;

export default meta;
type Story = StoryObj<typeof meta>;

function Rename({
  defaultOpen,
  long,
  fill = true,
}: {
  defaultOpen?: boolean;
  long?: boolean;
  /** As tall as the screen, for an overlay to open over; a matrix cell leaves it closed. */
  fill?: boolean;
}) {
  return (
    <div className={fill ? "min-h-[calc(100dvh-3rem)]" : undefined}>
      <DialogTrigger defaultOpen={defaultOpen}>
        <Button>Rename module</Button>
        <Dialog
          title={
            long
              ? "Rename the module Ground control: taxi instructions and hold-short readbacks"
              : "Rename module"
          }
        >
          {({ close }) => (
            <>
              <DialogBody>
                <TextField
                  label="Name"
                  defaultValue="Ground control"
                  description={
                    long
                      ? "Learners see the new name in their course list, in reminders and on certificates issued from now on. Certificates already issued keep the old name."
                      : "Learners see the new name at once."
                  }
                />
              </DialogBody>
              <DialogFooter>
                <Button onPress={close}>Cancel</Button>
                <Button intent="primary" onPress={close}>
                  Save name
                </Button>
              </DialogFooter>
            </>
          )}
        </Dialog>
      </DialogTrigger>
    </div>
  );
}

/** Closed: the button that opens it. */
export const Default: Story = { render: () => <Rename /> };

/** Open, over the dimmed and blurred page. */
export const Open: Story = { render: () => <Rename defaultOpen /> };

export const LongContent: Story = { render: () => <Rename defaultOpen long /> };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Rename fill={false} />
    </ThemeMatrix>
  ),
};
