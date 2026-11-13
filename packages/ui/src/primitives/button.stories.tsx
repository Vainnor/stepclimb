// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./button";

const meta = {
  title: "Primitives/Button",
  component: Button,
  args: { children: "Start quiz" },
  argTypes: {
    intent: { control: "inline-radio", options: ["primary", "secondary", "ghost", "danger"] },
    size: { control: "inline-radio", options: ["sm", "md", "lg"] },
    isDisabled: { control: "boolean" },
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

const themes = [
  { brand: "default", theme: "light", label: "Stepclimb default, light" },
  { brand: "default", theme: "dark", label: "Stepclimb default, dark" },
  { brand: "vzdc", theme: "light", label: "vZDC, light" },
  { brand: "vzdc", theme: "dark", label: "vZDC, dark" },
] as const;

/** Every intent in both example brands and both appearances, side by side. */
export const Themes: Story = {
  parameters: { layout: "fullscreen", themeWrapper: false },
  render: () => (
    <div className="grid grid-cols-1 sm:grid-cols-2">
      {themes.map((t) => (
        <section
          key={t.label}
          data-sc-brand={t.brand === "default" ? undefined : t.brand}
          data-sc-theme={t.theme}
          aria-label={t.label}
          className="flex flex-col gap-4 bg-canvas p-6 font-ui text-fg-default"
        >
          <h2 className="text-sm font-medium text-fg-sub">{t.label}</h2>
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
          </div>
        </section>
      ))}
    </div>
  ),
};
