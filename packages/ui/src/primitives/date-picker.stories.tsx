// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime } from "@internationalized/date";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { DatePicker } from "./date-picker";

const due = parseZonedDateTime("2026-03-08T09:00[America/New_York]");
// The empty field's segments start from a fixed day, so its zone name does not change with the
// day the story runs.
const placeholderValue = parseZonedDateTime("2026-03-02T00:00[America/New_York]");

const meta = {
  title: "Forms/DatePicker",
  component: DatePicker,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled date and time, such as a due date or an unlock date. People type into the segments (the arrow keys step one) or pick a day from the calendar, which opens in a popover at 768 px and wider and as a sheet from the bottom on phones. The value is a \`ZonedDateTime\`: an instant with its time zone, shown with the zone's short name ("EDT"), so a learner abroad sees when it is. React Aria works out the offset across a daylight-saving change.

**Usage.** \`<DatePicker label={t("assignment.due")} timeZone={me.tz} value={due} onChange={setDue} />\`. \`timeZone\` is the user's own (\`users.tz\`): a value typed into the empty field is in it. A value you pass keeps its own zone. \`granularity="day"\` hides the time.

**Do** pass the user's time zone, and send the server \`value.toAbsoluteString()\` or the value with its zone.

**Don't** convert the value to the browser's zone or to a date without a zone; don't use it for a time of day alone (that is a \`TimeField\`).`,
      },
    },
  },
  args: { label: "Due date", timeZone: "America/New_York", className: "max-w-sm" },
} satisfies Meta<typeof DatePicker>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { defaultValue: due, description: "Learners see it in their own time zone." },
};

export const Empty: Story = { args: { placeholderValue } };

/** Open: a popover at 1280 px, a bottom sheet at 375 px. */
export const Open: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <DatePicker {...args} defaultValue={due} defaultOpen />
    </div>
  ),
};

export const FocusVisible: Story = {
  args: { defaultValue: due },
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getAllByRole("spinbutton")[0]).toHaveFocus();
  },
};

export const DateOnly: Story = { args: { defaultValue: due, granularity: "day" } };

export const Invalid: Story = {
  args: {
    defaultValue: due,
    isInvalid: true,
    errorMessage: "Choose a due date after the assignment opens.",
  },
};

export const Disabled: Story = { args: { defaultValue: due, isDisabled: true } };

/** A long label, and a value in a zone with a long short name, at a narrow width. */
export const LongContent: Story = {
  args: {
    label: "When the recurrent training for this position must be completed again",
    defaultValue: parseZonedDateTime("2026-12-31T23:59[Australia/Lord_Howe]"),
    timeZone: "Australia/Lord_Howe",
    className: "max-w-64",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <DatePicker label="Due date" timeZone="America/New_York" defaultValue={due} />
      <DatePicker
        label="Due date"
        timeZone="America/New_York"
        placeholderValue={placeholderValue}
        isInvalid
        errorMessage="Choose a due date."
      />
      <DatePicker label="Due date" timeZone="America/New_York" defaultValue={due} isDisabled />
    </ThemeMatrix>
  ),
};
