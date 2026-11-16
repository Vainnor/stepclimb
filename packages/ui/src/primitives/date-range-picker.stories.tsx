// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime } from "@internationalized/date";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { DateRangePicker } from "./date-range-picker";

const openWindow = {
  start: parseZonedDateTime("2026-03-02T09:00[America/New_York]"),
  end: parseZonedDateTime("2026-03-13T17:00[America/New_York]"),
};
// The empty field's segments start from a fixed day, so its zone name does not change with the
// day the story runs.
const placeholderValue = parseZonedDateTime("2026-03-02T00:00[America/New_York]");

const meta = {
  title: "Forms/DateRangePicker",
  component: DateRangePicker,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled start and end, each a date and time with its time zone, typed into segments or picked as a range from a calendar (a popover at 768 px and wider, a sheet from the bottom on phones). Each end shows its zone's short name, which can differ when the range crosses a daylight-saving change.

**Usage.** \`<DateRangePicker label={t("assignment.window")} timeZone={me.tz} value={window} onChange={setWindow} />\`, where the value is \`{ start, end }\` of \`ZonedDateTime\`s. \`timeZone\` is the user's own (\`users.tz\`) for a range typed into the empty field.

**Do** check on the server that the end is after the start; set \`minValue\` to keep people from picking a past day where that is wrong.

**Don't** use two DatePickers for one range: the calendar here shows the whole range and keeps the end after the start.`,
      },
    },
  },
  args: { label: "Open window", timeZone: "America/New_York", className: "max-w-xl" },
} satisfies Meta<typeof DateRangePicker>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { defaultValue: openWindow } };

export const Empty: Story = { args: { placeholderValue } };

/** Open: a popover at 1280 px, a bottom sheet at 375 px. */
export const Open: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <DateRangePicker {...args} defaultValue={openWindow} defaultOpen />
    </div>
  ),
};

export const FocusVisible: Story = {
  args: { defaultValue: openWindow },
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getAllByRole("spinbutton")[0]).toHaveFocus();
  },
};

export const Invalid: Story = {
  args: {
    defaultValue: openWindow,
    isInvalid: true,
    errorMessage: "End the window at least a day after it opens.",
  },
};

export const Disabled: Story = { args: { defaultValue: openWindow, isDisabled: true } };

/** Narrow: the end wraps under the start. */
export const LongContent: Story = {
  args: {
    label: "The window in which learners can take the recurrent training this year",
    defaultValue: openWindow,
    className: "max-w-72",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <DateRangePicker label="Open window" timeZone="America/New_York" defaultValue={openWindow} />
      <DateRangePicker
        label="Open window"
        timeZone="America/New_York"
        placeholderValue={placeholderValue}
        isInvalid
        errorMessage="Choose when the window opens and closes."
      />
    </ThemeMatrix>
  ),
};
