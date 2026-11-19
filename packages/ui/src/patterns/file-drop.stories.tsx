// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { FileDrop } from "./file-drop";

const meta = {
  title: "Forms/FileDrop",
  component: FileDrop,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A zone to drop files on, with a button to choose them, for course file uploads and image insertion. It refuses a drag that carries no file, and a file of a type or size you do not accept, saying which file and why; it hands the rest to \`onSelect\`. A keyboard user can also paste a file into the zone. While a file uploads it shows the progress you report.

**Usage.** \`<FileDrop onSelect={startUpload} acceptedFileTypes={["application/pdf", "image/*"]} maxFileSize={limits.maxFileBytes} description={t("files.types")} upload={progress} errorMessage={uploadError} />\`.

**Do** pass the server's own size limit, and show the server's refusal in \`errorMessage\`. The zone's type and size checks only save people a wasted upload: the server refuses a file above the instance's size limit or over the organization's storage, and records the type it sniffs without refusing any.

**Don't** start the upload yourself on drop of a refused file, or hide the button: on a phone it is the only way to pick a file.`,
      },
    },
  },
  args: {
    onSelect: () => {},
    acceptedFileTypes: ["application/pdf", "image/*"],
    maxFileSize: 25_000_000,
    description: "A PDF or an image.",
    className: "max-w-lg",
  },
} satisfies Meta<typeof FileDrop>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Multiple: Story = { args: { allowsMultiple: true } };

export const Uploading: Story = { args: { upload: { name: "phraseology.pdf", fraction: 0.42 } } };

export const ServerError: Story = {
  args: {
    errorMessage:
      "The organization's storage is full. Delete files you no longer need, then try again.",
  },
};

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("button", { name: "Choose file" })).toHaveFocus();
  },
};

export const Disabled: Story = { args: { isDisabled: true } };

/**
 * Busy with the file just chosen: "Choose file" says it is in progress and takes no press, and
 * keeps the focus it had when the file picker closed.
 */
export const Busy: Story = {
  render: function Busy(args) {
    const [busy, setBusy] = useState(false);
    return <FileDrop {...args} isBusy={busy} onSelect={() => setBusy(true)} />;
  },
  play: async ({ canvasElement }) => {
    const choose = within(canvasElement).getByRole("button", { name: "Choose file" });
    await userEvent.tab();
    await userEvent.tab();
    await expect(choose).toHaveFocus();
    const input = canvasElement.querySelector<HTMLInputElement>('input[type="file"]');
    if (input === null) throw new Error("the file input");
    await userEvent.upload(input, new File(["%PDF"], "handout.pdf", { type: "application/pdf" }));
    await waitFor(() =>
      expect(choose.querySelector("[data-slot=button-pending]")).toHaveAttribute("data-shown"),
    );
    await expect(choose).toHaveAttribute("aria-disabled", "true");
    await expect(choose).toHaveFocus();
  },
};

export const LongContent: Story = {
  args: {
    description:
      "A PDF, a Word document, a PowerPoint presentation or an image in PNG, JPEG or WebP format.",
    upload: {
      name: "approach-control-standard-operating-procedures-2026-revision.pdf",
      fraction: 0.9,
    },
    className: "max-w-xs",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <FileDrop
        onSelect={() => {}}
        maxFileSize={25_000_000}
        description="A PDF or an image."
        upload={{ name: "phraseology.pdf", fraction: 0.42 }}
      />
      <FileDrop onSelect={() => {}} errorMessage="The organization's storage is full." isDisabled />
    </ThemeMatrix>
  ),
};
