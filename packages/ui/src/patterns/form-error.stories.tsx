// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../primitives/button";
import { TextField } from "../primitives/text-field";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { FormError } from "./form-error";

const meta = {
  title: "Forms/FormError",
  component: FormError,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `Why a form's submission failed, at the top of the form, when the failure belongs to no one field: a refused sign-in, a rate limit, a server that could not be reached. A warning icon marks it, so color is not the only sign, and screen readers announce it when it appears.

**Usage.** \`{error ? <FormError code={unknownCode}>{message}</FormError> : null}\`, rendered only while there is an error.

**Do** say what happened and what to do next, and pass \`code\` only for a code the app has no message for.

**Don't** use it for a field's own error (that is the field's \`errorMessage\`), or keep it on screen after the person fixed the problem.`,
      },
    },
  },
  args: { children: "Your account is suspended. Ask an administrator to restore it." },
} satisfies Meta<typeof FormError>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const WithCode: Story = {
  args: {
    children:
      "Something went wrong while signing you in. Try again. If it keeps happening, give an administrator this code:",
    code: "auth.teapot",
  },
};

export const InAForm: Story = {
  render: () => (
    <div className="flex max-w-md flex-col gap-4">
      <FormError>Too many sign-in attempts from your network. Try again in 42 seconds.</FormError>
      <TextField label="Email" defaultValue="avery@example.test" />
      <Button intent="primary">Continue</Button>
    </div>
  ),
};

export const LongContent: Story = {
  args: {
    children:
      "Your organization lets this link set up only a password, which this version of Stepclimb cannot do in the browser yet. Ask an administrator to turn on passkeys, then open the link again.",
    code: "01900000-0000-7000-8000-000000000002.organization.membership_conflict",
  },
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <FormError code="auth.teapot">Something went wrong. Try again.</FormError>
    </ThemeMatrix>
  ),
};
