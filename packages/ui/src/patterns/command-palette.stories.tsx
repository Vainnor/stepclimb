// SPDX-License-Identifier: AGPL-3.0-or-later
import { BookOpenIcon, ChartBarIcon, GearIcon, MoonIcon, UsersIcon } from "@phosphor-icons/react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, waitFor, within } from "storybook/test";
import { Button } from "../primitives/button";
import { Kbd } from "../primitives/kbd";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { type Command, CommandPalette } from "./command-palette";

/** Waits until the open palette is drawn, so the screenshot has it. */
async function shown() {
  const palette = await within(document.body).findByRole("dialog");
  await waitFor(() => expect(palette).toBeVisible(), { timeout: 2000 });
}

const meta = {
  title: "Overlays/CommandPalette",
  component: CommandPalette,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["States", "Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `A search box over the app's commands, opened with ⌘K on Apple platforms and Ctrl+K elsewhere, from anywhere on the page. Typing narrows the list by label and keywords, ignoring case and accents. Up and Down move through the matches while focus stays in the search box; Enter runs the highlighted command and closes the palette. Escape clears the search, then closes the palette, and focus goes back where it was. It sits at the top of the screen and ends above the on-screen keyboard on a phone.

**Usage.** Render one near the root of the app: \`<CommandPalette commands={commands} onAction={(id) => run(id)} />\`. Each command is \`{ id, label, keywords?, icon?, isDisabled? }\`, with its text from the strings; when some commands have icons, the others keep the icon column so the labels line up. \`isOpen\` and \`onOpenChange\` control it, for a button that opens it.

**Do** name commands by what they do ("Go to grades", "Switch to dark theme"), and add the words people search for as keywords.

**Don't** list a command the person cannot run, or make the palette the only way to reach something.`,
      },
    },
  },
  args: { commands: [], onAction: () => {} },
  argTypes: { commands: { control: false } },
} satisfies Meta<typeof CommandPalette>;

export default meta;
type Story = StoryObj<typeof meta>;

const commands: Command[] = [
  { id: "courses", label: "Go to courses", keywords: ["catalog", "classes"], icon: BookOpenIcon },
  { id: "grades", label: "Go to grades", keywords: ["scores", "results"], icon: ChartBarIcon },
  { id: "people", label: "Go to people", keywords: ["members", "users"], icon: UsersIcon },
  { id: "settings", label: "Open settings", icon: GearIcon },
  { id: "theme", label: "Switch to dark theme", keywords: ["appearance"], icon: MoonIcon },
];

/** A button that opens the palette, and the shortcut that does the same. */
function Opener({
  list = commands,
  defaultOpen = false,
}: {
  list?: Command[];
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="flex items-center gap-3">
      <Button onPress={() => setOpen(true)}>Search commands</Button>
      <Kbd>Ctrl K</Kbd>
      <CommandPalette commands={list} isOpen={open} onOpenChange={setOpen} onAction={() => {}} />
    </div>
  );
}

/** Closed: the shortcut or the button opens it. */
export const Default: Story = { render: () => <Opener /> };

/** Open, with focus in the search box. */
export const Open: Story = {
  play: shown,
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Opener defaultOpen />
    </div>
  ),
};

/** Nothing to list: it says so under the search box. */
export const Empty: Story = {
  play: shown,
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Opener defaultOpen list={[]} />
    </div>
  ),
};

/** A command that cannot run now is listed but disabled. */
export const Disabled: Story = {
  play: shown,
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Opener
        defaultOpen
        list={[
          ...commands.slice(0, 2),
          { id: "export", label: "Export the grade report", isDisabled: true },
        ]}
      />
    </div>
  ),
};

export const LongContent: Story = {
  play: shown,
  render: () => (
    <div className="min-h-[calc(100dvh-3rem)]">
      <Opener
        defaultOpen
        list={[
          {
            id: "assign",
            label:
              "Assign Ground control check and Local control check to everyone in the S1 cohort",
            icon: UsersIcon,
          },
          ...commands,
          { id: "a", label: "Go to the training calendar" },
          { id: "b", label: "Go to the audit log" },
          { id: "c", label: "Go to roles and permissions" },
          { id: "d", label: "Go to identity providers" },
          { id: "e", label: "Go to branding" },
        ]}
      />
    </div>
  ),
};

/** The button that opens it in every brand, theme and density. */
export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <Opener />
    </ThemeMatrix>
  ),
};
