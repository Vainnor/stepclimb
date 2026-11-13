// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { ComboBox, ComboBoxItem } from "./combo-box";

const meta = {
  title: "Forms/ComboBox",
  component: ComboBox,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A text field that narrows a list of options as you type. At 768 px and wider the options open in a popover under the input. On phones the field is a button that opens a sheet from the bottom with the input at its top, so the options sit above the keyboard instead of under it. While options load from the server it shows a spinner, and an empty list says it is loading instead of that nothing matched.

**Usage.** \`<ComboBox label="Position" placeholder="Type a position"><ComboBoxItem id="gnd">Ground control</ComboBoxItem>…</ComboBox>\` filters its items itself. For options from the server, pass \`items\`, a render function, \`inputValue\` and \`onInputChange\`, and set \`isLoading\` while the request runs. \`value\` and \`onChange\` control the chosen option.

**Do** use it when the list is long enough that people search it: members, facilities, courses.

**Don't** use it for a short list (a \`Select\`) or let it accept free text unless the field allows a value that is not in the list (\`allowsCustomValue\`).`,
      },
    },
  },
  args: {
    label: "Position",
    placeholder: "Type a position",
    className: "max-w-sm",
    children: null,
  },
} satisfies Meta<typeof ComboBox>;

export default meta;
type Story = StoryObj<typeof meta>;

const positions = (
  <>
    <ComboBoxItem id="del">Clearance delivery</ComboBoxItem>
    <ComboBoxItem id="gnd">Ground control</ComboBoxItem>
    <ComboBoxItem id="twr">Local control</ComboBoxItem>
    <ComboBoxItem id="app">Approach control</ComboBoxItem>
  </>
);

interface Position {
  id: string;
  name: string;
}

/** The server has not answered yet. */
const noPositions: Position[] = [];

/**
 * Opens the list from the field's button with the keyboard: a pointer left over the field would
 * draw it hovered at some runs and not others. On a touch screen React Aria reveals the sheet once
 * the on-screen keyboard has opened for its input, or after 600 ms without one, so this waits.
 */
async function openList({ canvasElement }: { canvasElement: HTMLElement }) {
  within(canvasElement)
    .getByRole("button", { name: /Show suggestions|Position/ })
    .focus();
  await userEvent.keyboard("{Enter}");
  const list = await within(document.body).findByRole("listbox");
  await waitFor(() => expect(list).toBeVisible(), { timeout: 2000 });
}

export const Default: Story = {
  render: (args) => (
    <ComboBox {...args} description="Start typing to narrow the list.">
      {positions}
    </ComboBox>
  ),
};

export const WithValue: Story = {
  render: (args) => (
    <ComboBox {...args} defaultValue="gnd">
      {positions}
    </ComboBox>
  ),
};

/** Open: a popover at 1280 px, a bottom sheet with the input at 375 px. */
export const Open: Story = {
  play: openList,
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <ComboBox {...args} defaultInputValue="control">
        {positions}
      </ComboBox>
    </div>
  ),
};

/** Nothing matches what was typed. */
export const Empty: Story = {
  play: openList,
  render: (args) => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <ComboBox {...args} defaultInputValue="oceanic">
        {positions}
      </ComboBox>
    </div>
  ),
};

/** Options loading from the server: a spinner, and the empty list says so. */
export const Loading: Story = {
  play: openList,
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <ComboBox<Position>
        label="Position"
        className="max-w-sm"
        defaultInputValue="gro"
        isLoading
        items={noPositions}
      >
        {(item) => <ComboBoxItem id={item.id}>{item.name}</ComboBoxItem>}
      </ComboBox>
    </div>
  ),
};

export const Invalid: Story = {
  render: (args) => (
    <ComboBox {...args} isInvalid errorMessage="Choose a position from the list.">
      {positions}
    </ComboBox>
  ),
};

export const Disabled: Story = {
  render: (args) => (
    <ComboBox {...args} isDisabled defaultValue="twr">
      {positions}
    </ComboBox>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <ComboBox label="Position" placeholder="Type a position">
        {positions}
      </ComboBox>
      <ComboBox label="Position" defaultValue="gnd" isInvalid errorMessage="Choose a position.">
        {positions}
      </ComboBox>
      <ComboBox label="Position" defaultValue="twr" isDisabled>
        {positions}
      </ComboBox>
    </ThemeMatrix>
  ),
};
