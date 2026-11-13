// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { Select, SelectItem } from "./select";

const meta = {
  title: "Forms/Select",
  component: Select,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A labeled field that picks one value from a short list. The options open in a popover under the field at 768 px and wider, and as a sheet from the bottom of the screen on phones. Arrow keys move through the options, typing jumps to one, Escape closes the list.

**Usage.** \`<Select label="Rating" value={rating} onChange={setRating}><SelectItem id="s1">S1 Ground</SelectItem>…</Select>\`. \`defaultValue\` leaves it uncontrolled. Without a value it shows "Choose one", or your \`placeholder\`.

**Do** use it for one choice from roughly 5 to 15 options. Order them in a way people expect: by rank, by date, or alphabetically.

**Don't** use it for two or three options (a \`RadioGroup\` shows them all), for long lists people search (a \`ComboBox\`), or for actions (a \`Menu\`).`,
      },
    },
  },
  args: { label: "Rating", className: "max-w-sm", children: null },
} satisfies Meta<typeof Select>;

export default meta;
type Story = StoryObj<typeof meta>;

const ratings = (
  <>
    <SelectItem id="s1">S1 Ground</SelectItem>
    <SelectItem id="s2">S2 Tower</SelectItem>
    <SelectItem id="s3" isDisabled>
      S3 Approach
    </SelectItem>
    <SelectItem id="c1">C1 Enroute</SelectItem>
  </>
);

export const Default: Story = {
  render: (args) => (
    <Select {...args} description="The position this course trains for.">
      {ratings}
    </Select>
  ),
};

export const WithValue: Story = {
  render: (args) => (
    <Select {...args} defaultValue="s2">
      {ratings}
    </Select>
  ),
};

/** Open: a popover at 1280 px, a bottom sheet at 375 px. */
export const Open: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Select {...args} defaultValue="s2" defaultOpen>
        {ratings}
      </Select>
    </div>
  ),
};

export const FocusVisible: Story = {
  render: (args) => <Select {...args}>{ratings}</Select>,
  play: async ({ canvasElement }) => {
    await userEvent.tab();
    await expect(within(canvasElement).getByRole("button")).toHaveFocus();
  },
};

export const Invalid: Story = {
  render: (args) => (
    <Select {...args} isInvalid errorMessage="Choose the rating this course trains for.">
      {ratings}
    </Select>
  ),
};

export const Disabled: Story = {
  render: (args) => (
    <Select {...args} isDisabled defaultValue="s1">
      {ratings}
    </Select>
  ),
};

export const LongContent: Story = {
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Select {...args} label="Training track" defaultValue="long" defaultOpen>
        <SelectItem id="long">
          Ground control and clearance delivery at a busy airport with parallel runways
        </SelectItem>
        <SelectItem id="short">Tower</SelectItem>
      </Select>
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Select label="Rating" description="The position this course trains for.">
        {ratings}
      </Select>
      <Select label="Rating" defaultValue="s2" isInvalid errorMessage="Choose a rating.">
        {ratings}
      </Select>
      <Select label="Rating" defaultValue="s1" isDisabled>
        {ratings}
      </Select>
    </ThemeMatrix>
  ),
};
