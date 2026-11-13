// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../primitives/button";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ErrorState } from "./error-state";

const meta = {
  title: "Data/ErrorState",
  component: ErrorState,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `What a view shows when it could not load: what happened as a heading, how to fix it, and a place for the retry action. A warning icon marks it, so color is not the only sign.

**Usage.** \`<ErrorState level={1} title={problem.title} action={<Button onPress={() => query.refetch()}>{t("error.retry")}</Button>}>{t("error.checkConnection")}</ErrorState>\`. \`level\` places the heading in the page outline.

**Do** use the server's problem title where there is one, and say what the person can do ("Check your connection, then try again").

**Don't** show a stack trace or an error code alone, or offer a retry that cannot help (a page that does not exist).`,
      },
    },
  },
  args: {
    level: 2,
    title: "The course could not be loaded",
    children: "Check your connection, then try again.",
  },
  argTypes: { action: { control: false } },
} satisfies Meta<typeof ErrorState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = { args: { action: <Button>Try again</Button> } };

/** Not found: no retry, since trying again cannot help. */
export const NotFound: Story = {
  args: {
    title: "Page not found",
    children: "Check the address, or go back to the home page.",
  },
};

export const LongContent: Story = {
  args: {
    title: "The grades for Ground control and Local control could not be loaded",
    children:
      "The server did not answer within 30 seconds. Wait a minute and try again; if it keeps happening, tell your training administrator.",
    action: <Button>Try again</Button>,
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ErrorState
        level={2}
        title="The course could not be loaded"
        action={<Button>Try again</Button>}
      >
        Check your connection, then try again.
      </ErrorState>
    </ThemeMatrix>
  ),
};
