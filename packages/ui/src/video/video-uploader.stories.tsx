// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ENDPOINT, MockTusServer, memoryStorage } from "../test/tus-server";
import { RESUME_KEY } from "./resume-store";
import type { UploadState } from "./use-video-upload";
import { VideoUploadCard, VideoUploader } from "./video-uploader";

const MB = 1_000_000;

/** A local storage holding one unfinished upload of the course "c1". */
function withUnfinished(name: string): Storage {
  const storage = memoryStorage();
  storage.setItem(
    RESUME_KEY,
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

**Usage.** \`<VideoUploader endpoint={\`/api/v1/courses/\${id}/video-uploads\`} storageKey={id} send={sendWithProgress} getVideo={readVideo} onChange={refreshList} />\`. \`send\` sends one request (the web app uses \`XMLHttpRequest\`, which reports upload progress); \`getVideo\` resolves \`null\` for a video that no longer exists.

**Do** keep the uploader mounted while the person stays on the page: unmounting aborts the chunk in flight. Refresh the course's video list in \`onChange\`.

**Don't** treat the file type check as a control: the server sniffs the bytes and fails anything that is not a video. Don't offer a second upload beside a running one; the uploader takes one at a time.`,
      },
    },
  },
  args: {
    endpoint: ENDPOINT,
    storageKey: "c1",
    send: server.send,
    getVideo: async () => null,
    storage: memoryStorage(),
    className: "max-w-lg",
  },
} satisfies Meta<typeof VideoUploader>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Unfinished: Story = {
  args: { storage: withUnfinished("Week 3 radar vectors.mp4") },
};

const base: UploadState = {
  phase: "uploading",
  name: "Week 1 phraseology.mp4",
  size: 840 * MB,
  sent: 352 * MB,
  origin: "new",
};

function Card({ state }: { state: UploadState }) {
  return (
    <div className="max-w-lg">
      <VideoUploadCard state={state} onPause={() => {}} onResume={() => {}} onCancel={() => {}} />
    </div>
  );
}

export const Uploading: Story = { render: () => <Card state={base} /> };

export const Interrupted: Story = {
  render: () => (
    <Card state={{ ...base, phase: "interrupted", problem: { kind: "unreachable" } }} />
  ),
};

export const Processing: Story = {
  render: () => <Card state={{ ...base, phase: "processing", sent: base.size }} />,
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
