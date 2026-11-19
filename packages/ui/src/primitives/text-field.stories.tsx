// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { TextField } from "./text-field";

const meta = {
  title: "Forms/TextField",
  component: TextField,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled one-line text input with help text and an error message. React Aria ties the label, the help and the error to the input, so screen readers read all three. On touch screens the text is 16 px, so iOS does not zoom in when the field is focused.

**Usage.** \`<TextField label={t("profile.displayName")} description={t("profile.displayNameHelp")} isRequired />\`. \`errorMessage\` replaces the browser's message; \`validate\` checks the value. \`value\` and \`onChange\` control it; \`defaultValue\` leaves it uncontrolled.

**Do** write the error as what is wrong and how to fix it ("Enter an email address with an @, such as name@example.org").

**Don't** use the placeholder as the label or for help that must stay readable, or say only "Invalid".`,
      },
    },
  },
  args: { label: "Display name", className: "max-w-sm" },
  argTypes: {
    isDisabled: { control: "boolean" },
    isInvalid: { control: "boolean" },
    isRequired: { control: "boolean" },
  },
} satisfies Meta<typeof TextField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithDescription: Story = {
  args: { description: "Your instructors and classmates see this name.", placeholder: "Ada" },
};

export const FocusVisible: Story = {
  args: { defaultValue: "Ada Lovelace" },
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("textbox")).toHaveFocus();
  },
};

/**
 * Hovers `target` until React Aria marks it hovered. It waits for the fonts and for the page to
 * settle first: after a layout change, Chromium sends a pointer event for wherever its own cursor
 * rests, which ends React Aria's hover. A screen without hover (a phone) shows the field focused
 * only.
 */
async function hover(target: HTMLElement) {
  if (window.matchMedia("(hover: none)").matches) return;
  await document.fonts.ready;
  await new Promise((resolve) => setTimeout(resolve, 300));
  await waitFor(async () => {
    await userEvent.unhover(target);
    await userEvent.hover(target);
    await expect(target).toHaveAttribute("data-hovered");
  });
}

/** Clicked into, with the pointer still over it: the focus border shows, not the hover one. */
export const FocusedHovered: Story = {
  args: { defaultValue: "Ada Lovelace" },
  play: async ({ canvasElement }) => {
    const input = within(canvasElement).getByRole("textbox");
    await userEvent.click(input);
    await hover(input);
    await expect(input).toHaveAttribute("data-focused");
  },
};

/** The error says what is wrong and how to fix it. */
export const Invalid: Story = {
  args: {
    label: "Email",
    defaultValue: "ada.example.org",
    isInvalid: true,
    errorMessage: "Enter an email address with an @, such as name@example.org.",
  },
};

export const Disabled: Story = { args: { isDisabled: true, defaultValue: "Ada Lovelace" } };

export const LongContent: Story = {
  args: {
    label: "The name you want on your certificate of completion, as it appears on your ID",
    description:
      "Certificates are checked against your ID by some facilities, so use the spelling on it, including any middle names.",
    defaultValue: "Augusta Ada King, Countess of Lovelace, née Byron",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <TextField label="Display name" description="Shown to your instructors." placeholder="Ada" />
      <TextField
        label="Email"
        defaultValue="ada.example.org"
        isInvalid
        errorMessage="Enter an email address with an @, such as name@example.org."
      />
      <TextField label="Organization" defaultValue="Example Training" isDisabled />
    </ThemeMatrix>
  ),
};
