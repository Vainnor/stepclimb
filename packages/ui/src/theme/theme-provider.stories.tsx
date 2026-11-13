// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../primitives/button";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ThemeProvider, useAppearance } from "./theme-provider";

function Sample() {
  const { theme, density } = useAppearance();
  return (
    <div className="flex flex-col gap-3 rounded-md border border-line bg-raised p-4">
      <p className="text-sm text-fg-sub">{`Theme ${theme}, density ${density}`}</p>
      <div className="flex flex-wrap gap-3">
        <Button intent="primary">Continue</Button>
        <Button>Preview</Button>
      </div>
    </div>
  );
}

const meta = {
  title: "Foundations/ThemeProvider",
  component: ThemeProvider,
  tags: ["experimental"],
  parameters: {
    themeWrapper: false,
    status: {
      missing: ["Themes", "Accessibility", "Responsive and touch", "Tests", "Docs"],
    },
  },
  args: { children: <Sample /> },
  argTypes: {
    theme: { control: "inline-radio", options: [undefined, "light", "dark", "high-contrast"] },
    density: { control: "inline-radio", options: ["comfortable", "compact"] },
    brand: { control: "inline-radio", options: [undefined, "vzdc"] },
    children: { control: false },
  },
} satisfies Meta<typeof ThemeProvider>;

export default meta;
type Story = StoryObj<typeof meta>;

/** No theme set: the provider follows the OS setting. */
export const FollowsSystem: Story = {
  render: (args) => (
    <ThemeProvider {...args}>
      <div className="min-h-screen bg-canvas p-6 font-ui text-fg-default">{args.children}</div>
    </ThemeProvider>
  ),
};

/** The same sample in each brand, theme and density. */
export const Matrix: Story = {
  render: () => (
    <ThemeMatrix>
      <Sample />
    </ThemeMatrix>
  ),
};
