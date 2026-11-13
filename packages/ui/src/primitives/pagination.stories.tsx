// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Pagination } from "./pagination";

const meta = {
  title: "Navigation/Pagination",
  component: Pagination,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Previous and Next through a list the server returns a page at a time. The server pages with cursors, which have no page numbers, so none are shown; the number of results shows when the API gives one. A direction with nowhere to go is disabled.

**Usage.** \`<Pagination previousCursor={page.prev} nextCursor={page.next} count={page.total} onPageChange={(cursor) => setCursor(cursor)} />\`.

**Do** put it under the list it pages, and keep the list's scroll position at the top after a page change.

**Don't** invent page numbers for a cursor-paged list.`,
      },
    },
  },
  args: { onPageChange: () => {} },
} satisfies Meta<typeof Pagination>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { previousCursor: "p", nextCursor: "n", count: 1204 } };

/** The first page: nothing before it. */
export const FirstPage: Story = { args: { nextCursor: "n", count: 48 } };

/** No count from the API. */
export const WithoutCount: Story = { args: { previousCursor: "p", nextCursor: "n" } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Pagination previousCursor="p" nextCursor="n" count={1204} onPageChange={() => {}} />
      <Pagination nextCursor="n" count={1} onPageChange={() => {}} />
    </ThemeMatrix>
  ),
};
