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
        component: `A labeled time of day, typed into segments or stepped with the arrow keys, followed by its time zone's short name. The value is a \`ZonedDateTime\`, so the time names an instant on a day; stepping the hour across a daylight-saving change gives the real next hour.

**Usage.** \`<TimeField label={t("reminders.sendAt")} timeZone={me.tz} value={sendAt} onChange={setSendAt} />\`. \`timeZone\` is the user's own (\`users.tz\`) for a time typed into the empty field.

**Do** use it where the day is known or fixed by the screen, and show which day that is.

**Don't** use it for a date and time together (that is a \`DatePicker\`) or for a duration (a \`NumberField\` with a unit).`,
      },
    },
  },
  args: { label: "Reminder time", timeZone: "America/Chicago", className: "max-w-48" },
} satisfies Meta<typeof TimeField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { defaultValue: nine, description: "Reminders go out at this time." },
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
    label: "The time of day the escalation email goes to the learner's manager",
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
        label="Reminder time"
        timeZone="America/Chicago"
        defaultValue={nine}
        className="max-w-48"
      />
      <TimeField
        label="Reminder time"
        timeZone="America/Chicago"
        placeholderValue={placeholderValue}
        isInvalid
        errorMessage="Choose a time."
        className="max-w-48"
      />
    </ThemeMatrix>
  ),
};
