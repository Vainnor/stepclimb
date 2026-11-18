// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ENDPOINT, MockTusServer, memoryStorage } from "../test/tus-server";
import { resumeKey } from "./resume-store";
import type { UploadState } from "./use-video-upload";
import { VideoUploadCard, VideoUploader } from "./video-uploader";

const MB = 1_000_000;

const OWNER = "o1.u1";

/** A local storage holding one unfinished upload of the course "c1". */
function withUnfinished(name: string): Storage {
  const storage = memoryStorage();
  storage.setItem(
    resumeKey(OWNER),
    JSON.stringify([
      {
        scope: "c1",
        fingerprint: "[]",
        url: "http://localhost/api/v1/video-uploads/u9",
        videoId: "v9",
        name,
        size: 840 * MB,
        offset: 352 * MB,
        // Far in the future, so the story never ages past the day an upload is kept.
        savedAt: 4_000_000_000_000,
      },
    ]),
  );
  return storage;
}

const server = new MockTusServer();

const meta = {
  title: "Authoring/VideoUploader",
  component: VideoUploader,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Uploads a video to a course through tus in 16 MiB chunks, so a large upload survives a dropped connection. It shows progress with Pause, Resume and Cancel; goes on by itself when the browser is back online; keeps each unfinished upload in local storage, so choosing the same file after a reload continues it; then reads the video every 10 seconds until it is ready or failed. A polite status region announces the phase and the progress in 10 % steps.

**Usage.** \`<VideoUploader endpoint={\`/api/v1/courses/\${id}/video-uploads\`} storageKey={id} owner={\`\${orgId}.\${userId}\`} send={sendWithProgress} getVideo={readVideo} onChange={refreshList} />\`. \`send\` sends one request (the web app uses \`XMLHttpRequest\`, which reports upload progress); \`getVideo\` resolves \`null\` for a video that no longer exists.

**Do** pass the signed-in organization and user as \`owner\`, and clear their entries with \`clearResumeEntries\` on an explicit sign-out. Keep the uploader mounted while the person stays on the page: unmounting aborts the chunk in flight. Refresh the course's video list in \`onChange\`.

**Don't** treat the file type check as a control: the server sniffs the bytes and fails anything that is not a video. Don't offer a second upload beside a running one; the uploader takes one at a time.`,
      },
    },
  },
  args: {
    endpoint: ENDPOINT,
    storageKey: "c1",
    owner: OWNER,
    send: server.send,
    getVideo: async () => null,
    storage: memoryStorage(),
    className: "max-w-lg",
  },
} satisfies Meta<typeof VideoUploader>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Disabled: Story = { args: { isDisabled: true } };

export const Unfinished: Story = {
  args: { storage: withUnfinished("Week 3 radar vectors.mp4") },
};

/** Discard on an upload the server holds bytes of asks first (D34). */
export const DiscardConfirm: Story = {
  args: {
    storage: withUnfinished("Week 3 radar vectors.mp4"),
    className: "max-w-lg min-h-[calc(100dvh-3rem)]",
  },
  play: async ({ canvasElement }) => {
    await userEvent.click(
      within(canvasElement).getByRole("button", {
        name: "Discard the upload of Week 3 radar vectors.mp4",
      }),
    );
    await expect(
      within(document.body).getByRole("alertdialog", {
        name: "Discard the upload of Week 3 radar vectors.mp4?",
      }),
    ).toBeVisible();
  },
};

const base: UploadState = {
  phase: "uploading",
  name: "Week 1 phraseology.mp4",
  size: 840 * MB,
  sent: 352 * MB,
  stored: 336 * MB,
  origin: "new",
};

function Card({ state }: { state: UploadState }) {
  return (
    <div className="max-w-lg">
      <VideoUploadCard state={state} onPause={() => {}} onResume={() => {}} onCancel={() => {}} />
    </div>
  );
}

export const Starting: Story = {
  render: () => <Card state={{ ...base, phase: "starting", sent: 0, stored: 0 }} />,
};

export const Uploading: Story = { render: () => <Card state={base} /> };

export const Resumed: Story = { render: () => <Card state={{ ...base, origin: "resumed" }} /> };

export const Restarted: Story = {
  render: () => <Card state={{ ...base, origin: "restarted", sent: 12 * MB, stored: 0 }} />,
};

export const Paused: Story = {
  render: () => <Card state={{ ...base, phase: "paused", sent: base.stored }} />,
};

export const Cancelling: Story = {
  render: () => <Card state={{ ...base, phase: "cancelling", sent: base.stored }} />,
};

export const Cancelled: Story = {
  render: () => <Card state={{ ...base, phase: "cancelled", sent: base.stored }} />,
};

/** Cancel on an upload the server holds bytes of asks first (D34). */
export const CancelConfirm: Story = {
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Card state={base} />
    </div>
  ),
  play: async ({ canvasElement }) => {
    await userEvent.click(within(canvasElement).getByRole("button", { name: "Cancel upload" }));
    await expect(
      within(document.body).getByRole("alertdialog", { name: "Cancel this upload?" }),
    ).toBeVisible();
  },
};

export const Interrupted: Story = {
  render: () => (
    <Card state={{ ...base, phase: "interrupted", problem: { kind: "unreachable" } }} />
  ),
};

export const Processing: Story = {
  render: () => <Card state={{ ...base, phase: "processing", sent: base.size }} />,
};

export const PollFailing: Story = {
  render: () => (
    <Card state={{ ...base, phase: "processing", sent: base.size, pollFailing: true }} />
  ),
};

export const Ready: Story = {
  render: () => <Card state={{ ...base, phase: "ready", sent: base.size }} />,
};

export const Failed: Story = {
  render: () => (
    <Card
      state={{
        ...base,
        phase: "failed",
        sent: base.size,
        problem: { kind: "video", code: "video.not_a_video" },
      }}
    />
  ),
};

export const Refused: Story = {
  render: () => (
    <Card
      state={{
        ...base,
        phase: "failed",
        sent: 0,
        problem: { kind: "refused", status: 403, code: "entitlement.limit_reached" },
      }}
    />
  ),
};

export const LongContent: Story = {
  render: () => (
    <div className="max-w-xs">
      <VideoUploadCard
        state={{
          ...base,
          phase: "paused",
          sent: base.stored,
          name: "approach-control-radar-vectoring-and-sequencing-lecture-recording-2026-11-revision.mp4",
          origin: "resumed",
          problem: { kind: "refused", status: 412, code: "tus.version_unsupported" },
        }}
      />
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <VideoUploadCard state={base} />
      <VideoUploadCard
        state={{ ...base, phase: "interrupted", problem: { kind: "unreachable" } }}
      />
      <VideoUploadCard state={{ ...base, phase: "ready", sent: base.size }} />
    </ThemeMatrix>
  ),
};
