// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Button } from "./button";

const meta = {
  title: "Primitives/Button",
  component: Button,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A button that names its action. Secondary by default; \`intent="primary"\` marks the one next thing to do in a view.

**Usage.** \`<Button intent="primary" onPress={submit}>Submit quiz</Button>\`. \`isPending\` shows the loading state: the button keeps its width and stays focusable, does not press, and screen readers hear its label and "In progress". A focused button announces when it turns pending and when it stops; if the view removes the button first, such as a sign-in step that moves on, the announcement leaves the page with it.

**Do** write the action as a verb ("Submit quiz", "Delete attempt"), keep one primary button per view, and use \`danger\` for actions that destroy something.

**Don't** write "OK" or "Click here", use \`isDisabled\` while a request runs (that is \`isPending\`), or change its color with \`className\`.`,
      },
    },
  },
  args: { children: "Start quiz" },
  argTypes: {
    intent: { control: "inline-radio", options: ["primary", "secondary", "ghost", "danger"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
    isPending: { control: "boolean" },
  },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = { args: { intent: "primary", children: "Start quiz" } };
export const Secondary: Story = { args: { intent: "secondary", children: "Preview" } };
export const Ghost: Story = { args: { intent: "ghost", children: "Skip" } };
export const Danger: Story = { args: { intent: "danger", children: "Delete attempt" } };
export const Small: Story = { args: { size: "sm", children: "Small" } };
export const Disabled: Story = { args: { isDisabled: true, children: "Disabled" } };

/** Waits out the 300 ms before a pending button draws its spinner, so the screenshot has it. */
async function spinnersShown({ canvasElement }: { canvasElement: HTMLElement }) {
  await waitFor(() => {
    const spinners = canvasElement.querySelectorAll('[data-slot="button-pending"]');
    expect(spinners.length).toBeGreaterThan(0);
    for (const s of spinners) expect(s).toHaveAttribute("data-shown");
  });
}

/**
 * Loading: after 300 ms the label turns invisible under a spinner, keeping the button's width and
 * its accessible name.
 */
export const Pending: Story = {
  args: { intent: "primary", isPending: true, children: "Submit quiz" },
  play: spinnersShown,
};

/**
 * A mouse press held on the button. A scripted press focuses it as a keyboard would, and the
 * browser then draws its focus ring; a real mouse press draws none, so the play blurs it.
 */
const press: Story["play"] = async ({ canvasElement }) => {
  const button = await within(canvasElement).findByRole("button");
  await userEvent.pointer({ keys: "[MouseLeft>]", target: button });
  await waitFor(() => expect(button).toHaveAttribute("data-pressed"));
  button.blur();
  await expect(button).toHaveAttribute("data-pressed");
};

/** Pressed: one step past hover in each intent. */
export const PressedPrimary: Story = {
  args: { intent: "primary", children: "Start quiz" },
  play: press,
};
export const PressedSecondary: Story = { args: { children: "Preview" }, play: press };
export const PressedGhost: Story = { args: { intent: "ghost", children: "Skip" }, play: press };
export const PressedDanger: Story = {
  args: { intent: "danger", children: "Delete attempt" },
  play: press,
};

export const FocusVisible: Story = {
  args: { children: "Preview" },
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("button")).toHaveAttribute("data-focus-visible");
  },
};

/** Every intent and size in each brand, theme and density, side by side. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  play: spinnersShown,
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-wrap items-center gap-3">
        <Button intent="primary">Start quiz</Button>
        <Button>Preview</Button>
        <Button intent="ghost">Skip</Button>
        <Button intent="danger">Delete attempt</Button>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Button size="sm">Small</Button>
        <Button isDisabled>Disabled</Button>
        <Button size="lg" intent="primary">
          Large
        </Button>
        <Button intent="primary" isPending>
          Submit quiz
        </Button>
      </div>
    </ThemeMatrix>
  ),
};
