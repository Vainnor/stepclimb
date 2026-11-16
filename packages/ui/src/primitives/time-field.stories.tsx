// SPDX-License-Identifier: AGPL-3.0-or-later
import { parseZonedDateTime } from "@internationalized/date";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { TimeField } from "./time-field";

const nine = parseZonedDateTime("2026-11-16T09:00[America/Chicago]");
// The empty field's segments start from a fixed day, so its zone name does not change with the
// day the story runs.
const placeholderValue = parseZonedDateTime("2026-11-16T00:00[America/Chicago]");

const meta = {
  title: "Forms/TimeField",
  component: TimeField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled time of day on a day the screen already fixes, typed into segments or stepped with the arrow keys, followed by its time zone's short name. The value is a \`ZonedDateTime\`, so the time names one instant on that day; stepping the hour across a daylight-saving change gives the real next hour.

**Usage.** For a practical session on 16 November 2026, shown above the field: \`<TimeField label={t("session.startsAt")} timeZone={me.tz} value={startsAt} onChange={setStartsAt} />\`, where \`startsAt\` is that day in the user's zone (\`users.tz\`). \`timeZone\` is the zone a time typed into the empty field gets.

**Do** say which day the time is on, next to the field.

**Don't** use it for a time that recurs every day or week, such as the hour reminders go out: a \`ZonedDateTime\` holds one day's UTC offset, so the time would move by an hour after a daylight-saving change. Store a wall-clock time and a zone for those. Don't use it for a date and time together (that is a \`DatePicker\`) or for a duration (a \`NumberField\` with a unit).`,
      },
    },
  },
  args: { label: "Session starts", timeZone: "America/Chicago", className: "max-w-48" },
} satisfies Meta<typeof TimeField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { defaultValue: nine, description: "On Monday, 16 November 2026." },
};

export const Empty: Story = { args: { placeholderValue } };

export const FocusVisible: Story = {
  args: { defaultValue: nine },
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getAllByRole("spinbutton")[0]).toHaveFocus();
  },
};

export const Invalid: Story = {
  args: {
    defaultValue: nine,
    isInvalid: true,
    errorMessage: "Choose a time between 08:00 and 18:00.",
  },
};

export const Disabled: Story = { args: { defaultValue: nine, isDisabled: true } };

export const LongContent: Story = {
  args: {
    label: "The time the practical assessment with the examiner starts on Monday",
    defaultValue: parseZonedDateTime("2026-11-16T21:45[Asia/Kathmandu]"),
    timeZone: "Asia/Kathmandu",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <TimeField
        label="Session starts"
        timeZone="America/Chicago"
        defaultValue={nine}
        className="max-w-48"
      />
      <TimeField
        label="Session starts"
        timeZone="America/Chicago"
        placeholderValue={placeholderValue}
        isInvalid
        errorMessage="Choose a time."
        className="max-w-48"
      />
    </ThemeMatrix>
  ),
};
