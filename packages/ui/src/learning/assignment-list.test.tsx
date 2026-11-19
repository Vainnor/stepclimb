// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { AssignmentList, type AssignmentListEntry, sortAssignments } from "./assignment-list";

const NY = "America/New_York";
const now = new Date("2026-03-10T14:00:00Z");

const entry = (over: Partial<AssignmentListEntry> & { id: string }): AssignmentListEntry => ({
  target: { type: "course", title: `Course ${over.id}` },
  href: `/c/${over.id}`,
  due: null,
  status: "not_started",
  ...over,
});

const rows = () => screen.getAllByRole("listitem");
const titles = () => rows().map((row) => within(row).getByRole("link").textContent);

describe("sortAssignments", () => {
  it("puts overdue rows first, then rows by due date, then rows without one, keeping ties", () => {
    const sorted = sortAssignments([
      entry({ id: "none-1" }),
      entry({ id: "late", due: new Date("2026-03-12T00:00:00Z") }),
      entry({ id: "overdue-new", status: "overdue", due: new Date("2026-03-09T00:00:00Z") }),
      entry({ id: "soon", due: new Date("2026-03-11T00:00:00Z"), status: "in_progress" }),
      entry({ id: "none-2", status: "completed" }),
      entry({ id: "overdue-old", status: "overdue", due: new Date("2026-03-01T00:00:00Z") }),
      // Completed before an overdue row's date: still after every overdue row.
      entry({ id: "done", status: "completed", due: new Date("2026-02-01T00:00:00Z") }),
    ]);
    expect(sorted.map((e) => e.id)).toEqual([
      "overdue-old",
      "overdue-new",
      "done",
      "soon",
      "late",
      "none-1",
      "none-2",
    ]);
  });
});

describe("AssignmentList", () => {
  it("lists the rows overdue first, each a link named by its target's title", () => {
    renderWithStrings(
      <AssignmentList
        timeZone={NY}
        now={now}
        assignments={[
          entry({ id: "1", target: { type: "course", title: "S1 Ground theory" } }),
          entry({
            id: "2",
            target: { type: "module", title: "Clearance delivery" },
            status: "overdue",
            due: new Date("2026-03-07T04:59:59Z"),
          }),
        ]}
      />,
    );
    expect(titles()).toEqual(["Clearance delivery", "S1 Ground theory"]);
    const link = screen.getByRole("link", { name: "Clearance delivery" });
    expect(link).toHaveAttribute("href", "/c/2");
  });

  it("describes each link with what was assigned, its course, its state and its due label", () => {
    renderWithStrings(
      <AssignmentList
        timeZone={NY}
        now={now}
        assignments={[
          entry({
            id: "1",
            target: { type: "item", title: "Taxi instructions" },
            courseTitle: "S1 Ground theory",
            status: "in_progress",
            due: new Date("2026-03-13T14:00:00Z"),
          }),
        ]}
      />,
    );
    const link = screen.getByRole("link", { name: "Taxi instructions" });
    expect(link).toHaveAccessibleDescription("Item, S1 Ground theory, In progress, Due in 3 days");
    const row = rows()[0] as HTMLElement;
    expect(row).toHaveAttribute("data-status", "in_progress");
    expect(row.querySelector('[data-slot="status-chip"]')).toHaveAttribute(
      "data-state",
      "progress",
    );
    expect(row.querySelector('[data-slot="due-badge"]')).toHaveTextContent("Due in 3 days");
  });

  it.each([
    ["not_started", "available", "Available"],
    ["in_progress", "progress", "In progress"],
    ["completed", "complete", "Complete"],
    ["overdue", "overdue", "Overdue"],
  ] as const)("draws %s as the %s learning state", (status, state, label) => {
    renderWithStrings(
      <AssignmentList timeZone={NY} now={now} assignments={[entry({ id: "1", status })]} />,
    );
    const chip = document.querySelector('[data-slot="status-chip"]');
    expect(chip).toHaveAttribute("data-state", state);
    expect(chip).toHaveTextContent(label);
  });

  it("shows the due date until the row is complete, and the overdue state once past it", () => {
    const due = new Date("2026-03-07T14:00:00Z");
    const { rerender } = renderWithStrings(
      <AssignmentList
        timeZone={NY}
        now={now}
        assignments={[entry({ id: "1", status: "overdue", due })]}
      />,
    );
    const badge = document.querySelector('[data-slot="due-badge"]');
    expect(badge).toHaveTextContent("3 days overdue");
    expect(badge).toHaveAttribute("data-overdue", "true");
    rerender(
      <AssignmentList
        timeZone={NY}
        now={now}
        assignments={[entry({ id: "1", status: "completed", due })]}
      />,
    );
    expect(document.querySelector('[data-slot="due-badge"]')).toBeNull();
    expect(screen.getByRole("link")).toHaveAccessibleDescription("Course, Complete");
  });

  it("draws the warning icon once on an overdue row, and the calendar on the others", () => {
    renderWithStrings(
      <AssignmentList
        timeZone={NY}
        now={now}
        assignments={[
          entry({ id: "1", status: "overdue", due: new Date("2026-03-07T14:00:00Z") }),
          entry({ id: "2", status: "in_progress", due: new Date("2026-03-13T14:00:00Z") }),
        ]}
      />,
    );
    const [overdue, soon] = rows() as [HTMLElement, HTMLElement];
    expect(overdue.querySelectorAll("svg")).toHaveLength(1);
    expect(overdue.querySelector('[data-slot="due-badge"] svg')).toBeNull();
    expect(soon.querySelector('[data-slot="due-badge"] svg')).not.toBeNull();
  });

  it("names a course target with the organization's term", () => {
    render(
      <StringsProvider
        catalogs={[uiMessages]}
        terms={{ course: { one: "training module", other: "training modules" } }}
        isDevelopment
      >
        <AssignmentList timeZone={NY} now={now} assignments={[entry({ id: "1" })]} />
      </StringsProvider>,
    );
    expect(screen.getByRole("link")).toHaveAccessibleDescription("Training module, Available");
  });

  it("says nothing is assigned when the list is empty", () => {
    renderWithStrings(<AssignmentList timeZone={NY} now={now} assignments={[]} />);
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.getByText("Nothing is assigned to you.")).toBeInTheDocument();
  });

  it.each<{ name: string; assignments: AssignmentListEntry[] }>([
    { name: "empty", assignments: [] },
    {
      name: "a row",
      assignments: [
        entry({
          id: "1",
          target: { type: "module", title: "«caller»" },
          courseTitle: "«caller»",
          status: "overdue",
          due: new Date(0),
        }),
      ],
    },
  ])("renders only catalog messages and the caller's content ($name)", ({ assignments }) => {
    const { container } = renderWithStrings(
      <AssignmentList timeZone="UTC" now={now} assignments={assignments} />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
