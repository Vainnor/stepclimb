// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import type { ItemRowItem } from "./item-row";
import { ModuleRow } from "./module-row";
import { learningStates } from "./status-chip";

const NY = "America/New_York";
const now = new Date("2026-03-10T14:00:00Z");

const items: ItemRowItem[] = [
  { id: "a", kind: "page", title: "Taxi routes", href: "/i/a", state: "complete" },
  { id: "h", kind: "header", title: "Practice" },
  { id: "b", kind: "video", title: "Taxi instructions", href: "/i/b", state: "progress" },
];

describe("ModuleRow", () => {
  it("is headed by its number and title", () => {
    renderWithStrings(
      <ModuleRow
        number={2}
        title="Ground control"
        state="progress"
        items={items}
        timeZone={NY}
        now={now}
      />,
    );
    expect(screen.getByRole("heading", { level: 2, name: "2 Ground control" })).toBeInTheDocument();
  });

  it("lists its items in order, with header items one heading level down", () => {
    renderWithStrings(
      <ModuleRow
        number={2}
        title="Ground control"
        state="progress"
        items={items}
        timeZone={NY}
        now={now}
        headingLevel={3}
      />,
    );
    const list = screen.getByRole("list");
    expect(list.tagName).toBe("OL");
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("Taxi routes"),
      "Practice",
      expect.stringContaining("Taxi instructions"),
    ]);
    expect(screen.getByRole("heading", { level: 4, name: "Practice" })).toBeInTheDocument();
  });

  it("shows progress as 3 / 5, read as 3 of 5 complete", () => {
    renderWithStrings(
      <ModuleRow
        number={1}
        title="Clearance delivery"
        state="progress"
        items={items}
        progress={{ completed: 3, required: 5 }}
        timeZone={NY}
        now={now}
      />,
    );
    const progress = document.querySelector('[data-slot="module-row-progress"]');
    expect(progress?.querySelector('[aria-hidden="true"]')).toHaveTextContent("3 / 5");
    expect(within(progress as HTMLElement).getByText("3 of 5 complete")).toBeInTheDocument();
  });

  it.each(learningStates.filter((s) => s !== "locked"))("shows the %s state's chip", (state) => {
    renderWithStrings(
      <ModuleRow
        number={1}
        title="Clearance delivery"
        state={state}
        items={[]}
        timeZone={NY}
        now={now}
      />,
    );
    const chip = document.querySelector('[data-slot="status-chip"]');
    expect(chip).toHaveAttribute("data-state", state);
    expect(document.querySelector('[data-slot="module-row"]')).toHaveAttribute("data-state", state);
  });

  it("while locked, says what it waits for in the user's zone, and no progress", () => {
    renderWithStrings(
      <ModuleRow
        number={3}
        title="Local control"
        state="locked"
        lockReasons={[
          { kind: "prerequisites", moduleTitles: ["Ground control"] },
          { kind: "unlock_at", at: new Date("2026-03-03T15:00:00Z") },
        ]}
        items={[]}
        progress={{ completed: 0, required: 4 }}
        timeZone={NY}
        now={now}
      />,
    );
    expect(document.querySelector('[data-slot="module-row-reasons"]')).toHaveTextContent(
      "Unlocks after Ground control, Opens on Mar 3, 2026, 10:00 AM EST",
    );
    expect(screen.getByText("Locked")).toBeInTheDocument();
    expect(document.querySelector('[data-slot="module-row-progress"]')).toBeNull();
  });

  it("shows the empty state when it has no items, and no list", () => {
    renderWithStrings(
      <ModuleRow
        number={1}
        title="Clearance delivery"
        state="available"
        items={[]}
        emptyState="Nothing here yet."
        timeZone={NY}
        now={now}
      />,
    );
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.getByText("Nothing here yet.")).toHaveAttribute("data-slot", "module-row-empty");
  });

  it("renders only catalog messages and the caller's content", () => {
    const { container } = renderWithStrings(
      <>
        <ModuleRow
          number={1}
          title="«caller»"
          state="progress"
          items={[{ id: "a", kind: "page", title: "«caller»", href: "/a", state: "complete" }]}
          progress={{ completed: 1, required: 2 }}
          timeZone="UTC"
          now={now}
        />
        <ModuleRow
          number={2}
          title="«caller»"
          state="locked"
          lockReasons={[{ kind: "prerequisites", moduleTitles: ["«caller»"] }]}
          items={[]}
          timeZone="UTC"
          now={now}
        />
      </>,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
