// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { TextField as AriaTextField, Input } from "react-aria-components";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { FieldDescription, FieldError, FieldLabel, field } from "./field";

const meta = {
  title: "Forms/Field",
  component: FieldLabel,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `The parts every form field shares: \`FieldLabel\`, \`FieldDescription\` and \`FieldError\`, laid out label, input, help, error; and \`field\`, the recipe for the bordered well an input sits in. \`TextField\`, \`TextArea\`, \`NumberField\` and \`SearchField\` are built from them; reach for them only to build a new kind of field.

**Usage.** Inside a React Aria field: \`<FieldLabel>…</FieldLabel><Input className={field().control()} /><FieldDescription>…</FieldDescription><FieldError />\`. \`FieldError\` renders only while the field is invalid, after a warning icon, and shows the validation errors when it has no text of its own.

**Do** use the finished fields where one fits.

**Don't** lay out a label or an error by hand: screen readers would not hear them with the input.`,
      },
    },
  },
} satisfies Meta<typeof FieldLabel>;

export default meta;
type Story = StoryObj<typeof meta>;

const styles = field();

function Callsign({ isInvalid }: { isInvalid?: boolean }) {
  return (
    <AriaTextField isInvalid={isInvalid} defaultValue="DAL 123" className={styles.root()}>
      <FieldLabel>Callsign</FieldLabel>
      <Input className={`${styles.control()} ${styles.input()}`} />
      <FieldDescription>As filed, for example DAL123.</FieldDescription>
      <FieldError>Use letters and digits only, with no spaces, such as DAL123.</FieldError>
    </AriaTextField>
  );
}

export const Parts: Story = { render: () => <Callsign /> };

export const Invalid: Story = { render: () => <Callsign isInvalid /> };

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Callsign isInvalid />
    </ThemeMatrix>
  ),
};
