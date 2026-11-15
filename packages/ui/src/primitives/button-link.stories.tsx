// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ButtonLink } from "./button-link";

const meta = {
  title: "Primitives/ButtonLink",
  component: ButtonLink,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A link drawn as a \`Button\`, with the same intents and sizes, for navigation that reads as an action: a sign-in provider's button. It stays a link: the browser shows its address, it opens in a new tab, and an in-app \`href\` goes through the app's \`RouterProvider\`, which \`routerOptions\` reach (the web app's \`{ reloadDocument: true }\` loads the address as a new page). A long label wraps and the control grows.

**Usage.** \`<ButtonLink href="/auth/example/start" routerOptions={{ reloadDocument: true }}>Sign in with Example</ButtonLink>\`.

**Do** use it when pressing it goes somewhere, and name where.

**Don't** use it for an action that stays on the page (that is a \`Button\`), or change its color with \`className\`.`,
      },
    },
  },
  args: { href: "/auth/example/start", children: "Sign in with Example" },
  argTypes: {
    intent: { control: "inline-radio", options: ["primary", "secondary", "ghost", "danger"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
  },
} satisfies Meta<typeof ButtonLink>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Secondary: Story = {};
export const Primary: Story = { args: { intent: "primary" } };
export const Disabled: Story = { args: { isDisabled: true } };

export const FocusVisible: Story = {
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("link")).toHaveAttribute("data-focus-visible");
  },
};

/** A configured label longer than the width wraps inside the control. */
export const LongContent: Story = {
  render: (args) => (
    <div className="flex max-w-xs flex-col">
      <ButtonLink {...args}>
        Sign in with the Example Regional Training Network single sign-on
      </ButtonLink>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col gap-3">
        <ButtonLink href="/auth/example/start" intent="primary">
          Sign in with Example
        </ButtonLink>
        <ButtonLink href="/auth/other/start">Sign in with Other</ButtonLink>
        <ButtonLink href="/auth/off/start" isDisabled>
          Sign in with Off
        </ButtonLink>
      </div>
    </ThemeMatrix>
  ),
};
