// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { CopyButton } from "./copy-button";
import { Kbd } from "./kbd";

/** Replaces the Clipboard API for one story, so its result does not depend on the browser. */
function clipboard(writeText: () => Promise<void>) {
  return () => {
    const original = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    return () => {
      if (original) Object.defineProperty(navigator, "clipboard", original);
      else Reflect.deleteProperty(navigator, "clipboard");
    };
  };
}

const meta = {
  title: "Actions/CopyButton",
  component: CopyButton,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Copies a value to the clipboard. For two seconds afterwards its icon shows the result, and a live region tells screen readers: "Copied", or that the copy failed and to copy the text by hand.

**Usage.** \`<CopyButton value={inviteLink} label={t("invite.copyLink")} />\`, beside the value it copies. Without \`label\` it is named "Copy".

**Do** put it next to the text it copies, so a person can still select that text when the browser refuses.

**Don't** copy something that is not shown on the page, or hide the value behind the button.`,
      },
    },
  },
  args: { value: "ZDC-4417-K2" },
} satisfies Meta<typeof CopyButton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  render: (args) => (
    <div className="flex items-center gap-2">
      <Kbd>{args.value}</Kbd>
      <CopyButton {...args} />
    </div>
  ),
};

/** After a copy: the check, and "Copied" in the live region. */
export const Copied: Story = {
  ...Default,
  args: { resetAfterMs: 600_000 },
  beforeEach: clipboard(async () => undefined),
  play: async ({ canvasElement }) => {
    await userEvent.click(within(canvasElement).getByRole("button", { name: "Copy" }));
    await expect(within(canvasElement).getByRole("status")).toHaveTextContent("Copied");
  },
};

/** When the browser refuses: a warning, and how to copy instead. */
export const Failed: Story = {
  ...Default,
  args: { resetAfterMs: 600_000 },
  beforeEach: clipboard(async () => {
    throw new DOMException("Write permission denied.", "NotAllowedError");
  }),
  play: async ({ canvasElement }) => {
    await userEvent.click(within(canvasElement).getByRole("button", { name: "Copy" }));
    await expect(within(canvasElement).getByRole("status")).toHaveTextContent(/^Could not copy/);
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex items-center gap-2">
        <Kbd>ZDC-4417-K2</Kbd>
        <CopyButton value="ZDC-4417-K2" />
        <CopyButton value="ZDC-4417-K2" isDisabled />
      </div>
    </ThemeMatrix>
  ),
};
