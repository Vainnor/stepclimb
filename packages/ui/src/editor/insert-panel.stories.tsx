// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ImagePanel, VideoPanel } from "./insert-panel";

const png = () =>
  new File([new Uint8Array([137, 80, 78, 71])], "taxiway-diagram.png", { type: "image/png" });

/**
 * Chooses the file, then moves the pointer off the drop zone: the upload leaves the pointer over
 * it, and its hover border would make the screenshot depend on where the pointer was.
 */
async function upload(canvasElement: HTMLElement) {
  const input = canvasElement.querySelector<HTMLInputElement>("input[type=file]");
  const zone = canvasElement.querySelector<HTMLElement>('[data-slot="file-drop"]');
  if (input) await userEvent.upload(input, png());
  if (zone) await userEvent.unhover(zone);
}

const meta = {
  title: "Authoring/InsertPanel",
  component: ImagePanel,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The details the page editor asks for before it inserts an image or a video, in a panel inside the editor rather than a modal: an image's file and text alternative, a video's YouTube or Vimeo address and title. The editor opens it from "Insert" or the slash menu.

**Usage.** Drawn by \`Editor\`; not used on its own.

**Do** describe what the image shows for someone who cannot see it, or mark it decorative.

**Don't** paste an embed code: the panel takes the video's address and keeps only its id.`,
      },
    },
  },
  args: {
    onUpload: () => new Promise(() => undefined),
    onInsert: () => undefined,
    onCancel: () => undefined,
  },
} satisfies Meta<typeof ImagePanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Image: Story = {};

/** A file on its way to the server. */
export const ImageUploading: Story = {
  play: async ({ canvasElement }) => {
    await upload(canvasElement);
    await expect(await within(canvasElement).findByText(/Uploading/)).toBeInTheDocument();
  },
};

/** The server read the file as something other than an image (spec 07 B10a). */
export const ImageNotAnImage: Story = {
  args: { onUpload: async () => ({ course_file_id: "f", mime: "application/pdf" }) },
  play: async ({ canvasElement }) => {
    await upload(canvasElement);
    await expect(await within(canvasElement).findByText(/is not a PNG/)).toBeInTheDocument();
  },
};

/** An address that names no YouTube or Vimeo video, and no title. */
export const VideoInvalid: Story = {
  render: () => <VideoPanel onInsert={() => undefined} onCancel={() => undefined} />,
  play: async ({ canvasElement }) => {
    const panel = within(canvasElement);
    await userEvent.type(
      panel.getByRole("textbox", { name: /Video address/ }),
      "https://example.com/x",
    );
    await userEvent.click(panel.getByRole("button", { name: "Insert video" }));
    await expect(await panel.findByText("Enter a YouTube or Vimeo video address.")).toBeVisible();
  },
};
