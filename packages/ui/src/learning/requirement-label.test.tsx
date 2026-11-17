// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { type Requirement, RequirementLabel } from "./requirement-label";

describe("RequirementLabel", () => {
  const cases: [Requirement, string][] = [
    [{ kind: "view" }, "View"],
    [{ kind: "mark_done" }, "Mark as done"],
    [{ kind: "watch", min_percent: 80 }, "Watch 80%"],
    [{ kind: "watch", min_percent: 100 }, "Watch 100%"],
    [{ kind: "watch", min_percent: 1 }, "Watch 1%"],
  ];

  it.each(cases)("says %j as %s", (requirement, text) => {
    const { container } = renderWithStrings(<RequirementLabel requirement={requirement} />);
    const label = container.querySelector('[data-slot="requirement-label"]');
    expect(label).toHaveTextContent(text);
    expect(label).toHaveAttribute("data-kind", requirement.kind);
  });

  it("renders only catalog messages", () => {
    const { container } = renderWithStrings(
      <RequirementLabel requirement={{ kind: "watch", min_percent: 80 }} />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
