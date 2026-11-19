// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { DownloadButton } from "./download-button";

const downloadedAt = new Date("2026-03-03T15:00:00Z");

const meta = {
  title: "Learning/DownloadButton",
  component: DownloadButton,
  tags: ["experimental"],
  render: (args) => (
    <div className="max-w-2xl">
      <DownloadButton {...args} />
    </div>
  ),
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A course's download for offline reading, in four states: not downloaded (a Download button, the package's size, and the note that the download is stored in the browser without encryption), downloading (progress and Cancel), downloaded (when and how large, and Remove) and update available (the same with Update). A refusal or a failed download shows below in a live region, so a screen reader hears it.

The component draws what the caller reports. The caller fetches the package, asks for persistent storage, stores the files, measures progress, refuses a package over the size limit or the browser's free quota before fetching anything, and checks for updates. A package refused as too large is not offered again: Download stays disabled while that problem shows.

\`isDisabled\` is for when the app is offline (spec 15 B9): it turns off Download and Update, which need the server, and leaves Remove and Cancel, which do not. Until the caller has read IndexedDB and knows the course's state, it draws a \`Skeleton\` the size of the button in its place rather than guessing a state.

**Usage.** \`<DownloadButton state={{ kind: "none", sizeBytes: pkg.size_bytes }} timeZone={me.tz} onDownload={...} onCancel={...} onRemove={...} onUpdate={...} />\`; pass \`problem\` after a refusal or failure and clear it at the next attempt. Leave the component out when the organization switches downloads off (\`offline.downloads_enabled\`).

**Do** keep the note: learners must know the download is not encrypted.

**Don't** draw it for content the server did not put in the package.`,
      },
    },
  },
  args: {
    state: { kind: "none", sizeBytes: 48_200_000 },
    timeZone: "America/New_York",
    onDownload: fn(),
    onCancel: fn(),
    onRemove: fn(),
    onUpdate: fn(),
  },
} satisfies Meta<typeof DownloadButton>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Not downloaded, with the package's size. */
export const Default: Story = {};

export const Downloading: Story = {
  args: { state: { kind: "downloading", receivedBytes: 19_000_000, totalBytes: 48_200_000 } },
};

export const Downloaded: Story = {
  args: { state: { kind: "downloaded", downloadedAt, sizeBytes: 48_200_000 } },
};

export const UpdateAvailable: Story = {
  args: { state: { kind: "update", downloadedAt, sizeBytes: 48_200_000 } },
};

/** Refused before any byte was fetched: larger than one course's download may be. */
export const TooLarge: Story = {
  args: {
    state: { kind: "none", sizeBytes: 620_000_000 },
    problem: { kind: "tooLarge", sizeBytes: 620_000_000 },
  },
};

/** Refused before any byte was fetched: larger than the space the browser reports free. */
export const NoRoom: Story = {
  args: {
    state: { kind: "none", sizeBytes: 90_000_000 },
    problem: { kind: "noRoom", sizeBytes: 90_000_000 },
  },
};

/** Offline: Update needs the server and is off; Remove still works. */
export const Disabled: Story = {
  args: { state: { kind: "update", downloadedAt, sizeBytes: 48_200_000 }, isDisabled: true },
};

/** Stopped partway; what it stored was removed. */
export const Failed: Story = { args: { problem: { kind: "failed" } } };

const downloadButton = (canvasElement: HTMLElement) =>
  within(canvasElement).findByRole("button", { name: "Download for offline use" });

/**
 * Hovers `target` until React Aria marks it hovered: a hover sent as the story first renders can
 * arrive before React Aria listens, and is then lost. It waits for the fonts and then for the page
 * to settle first: after a layout change, Chromium sends a pointer event for wherever its own
 * cursor rests, which ends React Aria's hover. A screen without hover (a phone) shows the button
 * as it is.
 */
async function hover(target: HTMLElement) {
  if (window.matchMedia("(hover: none)").matches) return;
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 300));
  await waitFor(async () => {
    await userEvent.unhover(target);
    await userEvent.hover(target);
    await expect(target).toHaveAttribute("data-hovered");
  });
}

/** The pointer over Download. */
export const Hover: Story = {
  play: async ({ canvasElement }) => {
    await hover(await downloadButton(canvasElement));
  },
};

/** Keyboard focus on Download. */
export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    const target = await downloadButton(canvasElement);
    await userEvent.tab();
    await waitFor(() => expect(target).toHaveAttribute("data-focus-visible"));
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <div className="flex flex-col gap-4">
        <DownloadButton {...args} />
        <DownloadButton
          {...args}
          state={{ kind: "downloading", receivedBytes: 19_000_000, totalBytes: 48_200_000 }}
        />
        <DownloadButton
          {...args}
          state={{ kind: "update", downloadedAt, sizeBytes: 48_200_000 }}
          problem={{ kind: "failed" }}
        />
      </div>
    </ThemeMatrix>
  ),
};
