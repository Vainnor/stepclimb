// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";
import { DialogTrigger } from "./dialog";
import { Sheet, type SheetPlacement } from "./sheet";
import { TextArea } from "./text-area";
import { TextField } from "./text-field";

const meta = {
  title: "Overlays/Sheet",
  component: Sheet,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A panel for editing that slides over the page: at the inline end of the screen at 768 px and wider, from the bottom on phones. It keeps clear of the notch and the home indicator. Its content scrolls; its actions stay pinned under it. Focus stays inside until it closes; Escape, the close button or a press on the page behind closes it.

**Usage.** \`<DialogTrigger><Button>Edit page</Button><Sheet title="Edit page" footer={({ close }) => <Button intent="primary" onPress={close}>Save page</Button>}>…</Sheet></DialogTrigger>\`. \`placement\` is \`auto\` (the default), \`end\` or \`bottom\`. \`isOpen\` and \`onOpenChange\` control it.

**Do** use it for editing that needs the page in view: a page's settings, a question's options.

**Don't** use it for a yes-or-no question (that is an \`AlertDialog\`) or for long work such as grading, which has its own page.`,
      },
    },
  },
  args: { title: "", children: null },
} satisfies Meta<typeof Sheet>;

export default meta;
type Story = StoryObj<typeof meta>;

function EditPage({
  defaultOpen,
  placement,
  long,
  fill = true,
}: {
  defaultOpen?: boolean;
  placement?: SheetPlacement;
  long?: boolean;
  /** As tall as the screen, for an overlay to open over; a matrix cell leaves it closed. */
  fill?: boolean;
}) {
  return (
    <div className={fill ? "min-h-[calc(100dvh-3rem)]" : undefined}>
      <DialogTrigger defaultOpen={defaultOpen}>
        <Button>Edit page</Button>
        <Sheet
          title={
            long
              ? "Edit Taxi instructions: hold-short readbacks and crossing clearances"
              : "Edit page"
          }
          placement={placement}
          footer={({ close }) => (
            <>
              <Button onPress={close}>Cancel</Button>
              <Button intent="primary" onPress={close}>
                Save page
              </Button>
            </>
          )}
        >
          <div className="flex flex-col gap-4">
            <TextField label="Title" defaultValue="Taxi instructions" />
            <TextArea
              label="Summary"
              defaultValue={
                long
                  ? "How to issue a taxi route with hold-short instructions, what the readback must contain, and what to do when it does not. Ends with three practice exchanges and a short check."
                  : "Issuing a taxi route and checking the readback."
              }
            />
          </div>
        </Sheet>
      </DialogTrigger>
    </div>
  );
}

/** Closed: the button that opens it. */
export const Default: Story = { render: () => <EditPage /> };

/** Open: at the inline end at 1280 px, from the bottom at 375 px. */
export const Open: Story = { render: () => <EditPage defaultOpen /> };

/** From the bottom at every width. */
export const Bottom: Story = { render: () => <EditPage defaultOpen placement="bottom" /> };

export const LongContent: Story = { render: () => <EditPage defaultOpen long /> };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <EditPage fill={false} />
    </ThemeMatrix>
  ),
};
