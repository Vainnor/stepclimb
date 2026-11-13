// SPDX-License-Identifier: AGPL-3.0-or-later
import { TrayIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { EmptyState } from "../patterns/empty-state";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Avatar } from "./avatar";
import { DataList, DataListItem } from "./data-list";
import { Text } from "./text";

const meta = {
  title: "Data/DataList",
  component: DataList,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A list of records, one per row, for lists a person reads top to bottom: their courses, the people waiting to join. Arrow keys move between items. With \`selectionMode\` each item starts with a checkbox that Space toggles; with \`onAction\` a press or Enter opens the item. Items are at least 44 px tall on touch screens.

**Usage.** \`<DataList aria-label={t("admissions.title")} selectionMode="multiple" emptyState={<EmptyState>{t("admissions.empty")}</EmptyState>}><DataListItem id={a.id} textValue={a.name}>…</DataListItem></DataList>\`. Give \`textValue\` when an item's content is not plain text. \`selectedKeys\` and \`onSelectionChange\` control the selection.

**Do** lead each item with what names it, and keep secondary details on the same row.

**Don't** use it for records compared column by column (that is a \`DataTable\`), or nest cards inside it.`,
      },
    },
  },
  args: { "aria-label": "Waiting to join", children: [] },
  argTypes: { children: { control: false } },
} satisfies Meta<typeof DataList>;

export default meta;
type Story = StoryObj<typeof meta>;

const people = [
  { id: "1", name: "Ada Park", detail: "Asked to join on 3 November" },
  { id: "2", name: "Ben Ortiz", detail: "Asked to join on 5 November" },
  { id: "3", name: "Cleo Novak", detail: "Asked to join on 9 November" },
];

function Person({ name, detail }: { name: string; detail: string }) {
  return (
    <>
      <Avatar name={name} size="sm" />
      <span className="flex min-w-0 flex-col">
        <Text size="sm" tone="strong">
          {name}
        </Text>
        <Text size="caption" tone="sub">
          {detail}
        </Text>
      </span>
    </>
  );
}

export const Default: Story = {
  render: (args) => (
    <DataList {...args}>
      {people.map((p) => (
        <DataListItem key={p.id} id={p.id} textValue={p.name}>
          <Person {...p} />
        </DataListItem>
      ))}
    </DataList>
  ),
};

/** Several can be chosen; one is. */
export const Selectable: Story = {
  render: (args) => (
    <DataList {...args} selectionMode="multiple" defaultSelectedKeys={["2"]}>
      {people.map((p) => (
        <DataListItem key={p.id} id={p.id} textValue={p.name}>
          <Person {...p} />
        </DataListItem>
      ))}
    </DataList>
  ),
};

/** An item that cannot be chosen. */
export const Disabled: Story = {
  render: (args) => (
    <DataList {...args} selectionMode="multiple" disabledKeys={["3"]}>
      {people.map((p) => (
        <DataListItem key={p.id} id={p.id} textValue={p.name}>
          <Person {...p} />
        </DataListItem>
      ))}
    </DataList>
  ),
};

export const Empty: Story = {
  render: (args) => (
    <DataList
      {...args}
      emptyState={<EmptyState icon={TrayIcon}>No one is waiting to join.</EmptyState>}
    >
      {[]}
    </DataList>
  ),
};

export const LongContent: Story = {
  render: (args) => (
    <DataList {...args} selectionMode="multiple">
      <DataListItem id="1" textValue="Maximiliana Castellanos-Whitfield">
        <Person
          name="Maximiliana Castellanos-Whitfield"
          detail="Asked to join on 3 November from the Ground control check waiting list, with a note for the training staff"
        />
      </DataListItem>
      <DataListItem id="2" textValue="Ben Ortiz">
        <Person {...(people[1] as (typeof people)[number])} />
      </DataListItem>
    </DataList>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <DataList aria-label="Waiting to join" selectionMode="multiple" defaultSelectedKeys={["2"]}>
        {people.map((p) => (
          <DataListItem key={p.id} id={p.id} textValue={p.name}>
            <Person {...p} />
          </DataListItem>
        ))}
      </DataList>
    </ThemeMatrix>
  ),
};
