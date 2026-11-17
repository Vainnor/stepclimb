// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, waitFor } from "storybook/test";
import cover from "../storybook/course-cover-sample.svg";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { CourseHeader } from "./course-header";

const meta = {
  title: "Learning/CourseHeader",
  component: CourseHeader,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The top of the course home: the course's cover, title and code, the learner's progress as a bar and, for staff, the course's status. It has no controls. The cover is decorative, since the title names the course, and one that fails to load (an expired signed URL) is left out rather than drawn broken.

**Usage.** For a learner: \`<CourseHeader title={course.title} code={course.code} coverUrl={course.cover_url} progress={{ completed, required }} />\`. For staff, give \`status={course.status}\` where \`course.permissions\` holds \`course.view_all\`, and no \`progress\`. On the course home the header carries the progress bar, so leave \`NextUp\`'s \`progress\` out there.

**Do** keep it the page's one \`h1\`.

**Don't** add statistics or a second primary action to it.`,
      },
    },
  },
  args: {
    title: "S1 Ground theory",
    code: "S1-GND",
    coverUrl: cover,
    progress: { completed: 3, required: 7 },
  },
} satisfies Meta<typeof CourseHeader>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Waits for the cover, so the screenshot never catches it half loaded. */
async function coverLoaded(canvasElement: HTMLElement) {
  const images = [...canvasElement.querySelectorAll("img")];
  await waitFor(() => expect(images.every((i) => i.complete && i.naturalWidth > 0)).toBe(true));
}

/** What a learner sees. */
export const Default: Story = {
  play: async ({ canvasElement }) => coverLoaded(canvasElement),
};

/** What staff see: the status, and no progress of their own. */
export const Staff: Story = {
  args: { status: "draft", progress: undefined },
  play: async ({ canvasElement }) => coverLoaded(canvasElement),
};

/** No cover set, or one that failed to load. */
export const WithoutCover: Story = { args: { coverUrl: null } };

export const LongContent: Story = {
  args: {
    title:
      "S3 Approach control: radar vectoring, sequencing and the arrival flow into a busy terminal area",
    code: "S3-APP-RADAR-VECTORING-AND-SEQUENCING-2026",
    status: "archived",
  },
  play: async ({ canvasElement }) => coverLoaded(canvasElement),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: (args) => (
    <ThemeMatrix>
      <CourseHeader {...args} status="published" />
    </ThemeMatrix>
  ),
  play: async ({ canvasElement }) => coverLoaded(canvasElement),
};
