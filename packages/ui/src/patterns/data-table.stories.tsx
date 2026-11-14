// SPDX-License-Identifier: AGPL-3.0-or-later
import { UsersIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import type { Selection, SortDescriptor } from "react-aria-components";
import { expect, waitFor } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { DataTable, type DataTableColumn } from "./data-table";
import { EmptyState } from "./empty-state";

interface Learner {
  id: string;
  name: string;
  rating: string;
  course: string;
  hours: number;
}

const learners: Learner[] = [
  { id: "1", name: "Ada Park", rating: "S1", course: "Ground control", hours: 12 },
  { id: "2", name: "Ben Ortiz", rating: "S2", course: "Local control", hours: 40 },
  { id: "3", name: "Cleo Novak", rating: "S3", course: "Approach control", hours: 75 },
  { id: "4", name: "Dev Rao", rating: "S1", course: "Clearance delivery", hours: 6 },
  { id: "5", name: "Eli Brandt", rating: "C1", course: "Center", hours: 210 },
];

const columns: DataTableColumn<Learner>[] = [
  { id: "name", label: "Name", allowsSorting: true, render: (l) => l.name },
  { id: "rating", label: "Rating", render: (l) => l.rating },
  { id: "course", label: "Current course", allowsSorting: true, render: (l) => l.course },
  { id: "hours", label: "Hours", align: "end", allowsSorting: true, render: (l) => l.hours },
];

/** Waits out the 300 ms before the loading line appears, so the screenshot has it. */
async function loadingShown({ canvasElement }: { canvasElement: HTMLElement }) {
  await waitFor(() =>
    expect(canvasElement.querySelector('[data-slot="data-table-loading"]')).not.toBeNull(),
  );
}

const meta = {
  title: "Data/DataTable",
  component: DataTable,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A page of records from the server in rows and columns. Arrow keys move between rows and cells. A sortable column's header shows its direction and asks for a new sort on a press or Enter; screen readers hear which column is sorted and which way. With \`selectionMode\` each row starts with a checkbox (Space toggles it) and \`multiple\` adds a select-all box. Wider than the screen, the table scrolls sideways on its own. In a compact \`ThemeProvider\` its rows are shorter, for dense views.

The table does not sort or page its rows: the server does. While \`isLoading\` it is busy, after 300 ms it shows a loading line under the table (and says so to screen readers), and it never shows the empty state.

**Usage.** \`<DataTable aria-labelledby="learners-heading" columns={columns} rows={page.items} sortDescriptor={sort} onSortChange={setSort} isLoading={query.isFetching} emptyState={<EmptyState>{t("learners.empty")}</EmptyState>} pagination={{ previousCursor: page.prev, nextCursor: page.next, count: page.total, onPageChange: setCursor }} />\`. Each column is \`{ id, label, render, allowsSorting?, align?, isRowHeader? }\`; the first column names each row unless another sets \`isRowHeader\`.

**Do** right-align numbers (\`align: "end"\`), and keep a row's name in its first column.

**Don't** put a table inside a card, or use one for a list a person reads top to bottom (that is a \`DataList\`).`,
      },
    },
  },
  args: { columns: [], rows: [] },
  argTypes: { columns: { control: false }, rows: { control: false } },
} satisfies Meta<typeof DataTable>;

export default meta;
type Story = StoryObj<typeof meta>;

function Learners({
  rows = learners,
  selectable = false,
  isLoading,
}: {
  rows?: Learner[];
  selectable?: boolean;
  isLoading?: boolean;
}) {
  const [sort, setSort] = useState<SortDescriptor>({ column: "name", direction: "ascending" });
  const [selected, setSelected] = useState<Selection>(new Set(selectable ? ["2", "3"] : []));
  return (
    <DataTable
      aria-label="Learners"
      columns={columns}
      rows={rows}
      sortDescriptor={sort}
      onSortChange={setSort}
      selectionMode={selectable ? "multiple" : "none"}
      selectedKeys={selected}
      onSelectionChange={setSelected}
      isLoading={isLoading}
      emptyState={<EmptyState icon={UsersIcon}>No learners are enrolled yet.</EmptyState>}
      pagination={{ nextCursor: "next", count: 48, onPageChange: () => {} }}
    />
  );
}

/** Sorted by name, with Previous and Next under it. */
export const Default: Story = { render: () => <Learners /> };

/** Two rows selected; the select-all box shows that some are. */
export const Selectable: Story = { render: () => <Learners selectable /> };

/** The first page loading: the loading line under the table after 300 ms. */
export const Loading: Story = {
  play: loadingShown,
  render: () => <Learners rows={[]} isLoading />,
};

export const Empty: Story = { render: () => <Learners rows={[]} /> };

export const LongContent: Story = {
  render: () => (
    <Learners
      rows={[
        ...learners.slice(0, 2),
        {
          id: "9",
          name: "Maximiliana Castellanos-Whitfield",
          rating: "S3",
          course: "Approach control: sequencing arrivals into the Washington area airports",
          hours: 1204,
        },
      ]}
    />
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Learners rows={learners.slice(0, 3)} selectable />
    </ThemeMatrix>
  ),
};
