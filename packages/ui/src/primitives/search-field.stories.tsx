// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { SearchField } from "./search-field";

const meta = {
  title: "Forms/SearchField",
  component: SearchField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled search input. While it has text, a clear button labeled "Clear search" shows at its end; Escape clears it too, and Enter submits.

**Usage.** \`<SearchField label={t("courses.search")} onSubmit={search} />\`, or \`onChange\` to filter as the person types. \`value\` and \`onChange\` control it.

**Do** label it with what it searches ("Search courses"), even when the label is only for screen readers (\`aria-label\`).

**Don't** put a search button beside it (Enter searches), or use it to enter a value for a form.`,
      },
    },
  },
  args: { label: "Search courses", className: "max-w-sm" },
} satisfies Meta<typeof SearchField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = { args: { placeholder: "Course name or code" } };

/** With text: the clear button shows. */
export const WithValue: Story = { args: { defaultValue: "ground control" } };

export const Disabled: Story = { args: { isDisabled: true, defaultValue: "ground control" } };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <SearchField label="Search courses" placeholder="Course name or code" />
      <SearchField label="Search courses" defaultValue="ground control" />
    </ThemeMatrix>
  ),
};
