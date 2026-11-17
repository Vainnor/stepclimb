// SPDX-License-Identifier: AGPL-3.0-or-later
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeMatrix } from "../storybook/theme-matrix";
import { type Requirement, RequirementLabel } from "./requirement-label";

const requirements: Requirement[] = [
  { kind: "view" },
  { kind: "mark_done" },
  { kind: "watch", min_percent: 80 },
];

const meta = {
  title: "Learning/RequirementLabel",
  component: RequirementLabel,
  tags: ["experimental"],
  parameters: {
    status: { missing: ["Themes", "Accessibility", "Responsive and touch"] },
    docs: {
      description: {
        component: `What finishes an item, in words: "View", "Mark as done", "Watch 80%".

**Usage.** \`<RequirementLabel requirement={item.requirement} />\` for an item whose \`requirement\` is not null.

**Do** pass the requirement as the API sends it.

**Don't** write your own wording for a requirement next to it.`,
      },
    },
  },
  args: { requirement: { kind: "mark_done" } },
} satisfies Meta<typeof RequirementLabel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const AllKinds: Story = {
  render: () => (
    <div className="flex flex-col gap-2">
      {requirements.map((requirement) => (
        <RequirementLabel key={requirement.kind} requirement={requirement} />
      ))}
    </div>
  ),
};

export const Matrix: Story = {
  tags: ["theme-matrix"],
  parameters: { themeWrapper: false },
  render: () => (
    <ThemeMatrix>
      <div className="flex flex-col gap-2">
        {requirements.map((requirement) => (
          <RequirementLabel key={requirement.kind} requirement={requirement} />
        ))}
      </div>
    </ThemeMatrix>
  ),
};
