// SPDX-License-Identifier: AGPL-3.0-or-later
// RequirementLabel: what finishes an item, in words, such as "Mark as done" or "Watch 80%"
// (docs/specs/m1/16-learning-ui-components.md B10). The requirement kinds are those the API
// accepts today (docs/specs/m1/04-unlock-and-completion-rules.md B3); quizzes and activities add
// theirs with M2 and M3.
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";

/** An item's requirement, as the API's `Requirement` schema sends it. */
export type Requirement =
  | { kind: "view" }
  | { kind: "mark_done" }
  /** `min_percent` is 1 to 100. */
  | { kind: "watch"; min_percent: number };

export const requirementLabel = tv({
  base: "font-ui text-caption text-fg-sub",
});

export interface RequirementLabelProps {
  /** The item's requirement. An item without one shows no label. */
  requirement: Requirement;
  /** Layout only. */
  className?: string;
}

export function RequirementLabel({ requirement, className }: RequirementLabelProps) {
  const { t } = useStepclimbStrings();
  return (
    <span
      data-slot="requirement-label"
      data-kind={requirement.kind}
      className={requirementLabel({ className })}
    >
      {requirementText(t, requirement)}
    </span>
  );
}

function requirementText(t: ReturnType<typeof useStepclimbStrings>["t"], requirement: Requirement) {
  switch (requirement.kind) {
    case "view":
      return t("requirementLabel.view");
    case "mark_done":
      return t("requirementLabel.markDone");
    case "watch":
      return t("requirementLabel.watch", { share: requirement.min_percent / 100 });
  }
}
