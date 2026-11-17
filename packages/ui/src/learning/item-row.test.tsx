// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { ItemRow, type ItemRowItem, itemKindIcons } from "./item-row";
import { learningStates } from "./status-chip";

const NY = "America/New_York";
const now = new Date("2026-03-10T14:00:00Z");

const page: ItemRowItem = {
  id: "i1",
  kind: "page",
  title: "Radio basics",
  href: "/c/1/m/2/i/3",
  state: "available",
  requirement: { kind: "mark_done" },
};

function renderRow(item: ItemRowItem, options?: { marked?: boolean }) {
  return renderWithStrings(
    <ol>
      <ItemRow item={item} timeZone={NY} now={now} />
    </ol>,
    options,
  );
}

describe("ItemRow", () => {
  it("links the title to the item view, named by the title alone", () => {
    renderRow(page);
    const link = screen.getByRole("link");
    expect(link).toHaveAccessibleName("Radio basics");
    expect(link).toHaveAttribute("href", "/c/1/m/2/i/3");
    expect(link).toHaveAttribute("data-slot", "item-row-link");
  });

  it("describes the link with the kind, requirement, state and due label, in that order", () => {
    renderRow({ ...page, due: new Date("2026-03-13T14:00:00Z") });
    expect(screen.getByRole("link")).toHaveAccessibleDescription(
      "Page Mark as done Available Due in 3 days",
    );
  });

  it("leaves out the requirement and due label it was not given", () => {
    renderRow({ ...page, requirement: null, state: "complete" });
    expect(screen.getByRole("link")).toHaveAccessibleDescription("Page Complete");
    expect(document.querySelector('[data-slot="requirement-label"]')).toBeNull();
    expect(document.querySelector('[data-slot="due-badge"]')).toBeNull();
  });

  it("draws each kind's own icon", () => {
    const kinds = Object.keys(itemKindIcons) as (keyof typeof itemKindIcons)[];
    const drawings = new Set<string>();
    for (const kind of kinds) {
      const { container, unmount } = renderRow({ ...page, kind });
      const icon = container.querySelector('[data-slot="item-row-link"] svg');
      expect(icon).toHaveAttribute("aria-hidden", "true");
      drawings.add(icon?.innerHTML ?? "");
      unmount();
    }
    expect(drawings.size).toBe(kinds.length);
  });

  it.each(learningStates)("shows the %s state as a chip with its label", (state) => {
    renderRow({ ...page, state });
    const row = screen.getByRole("listitem");
    expect(row).toHaveAttribute("data-state", state);
    expect(
      within(row).getByText(/./, { selector: '[data-slot="status-chip-label"]' }),
    ).toBeVisible();
  });

  it("keeps a locked item a link to its item view", () => {
    renderRow({ ...page, state: "locked" });
    const link = screen.getByRole("link", { name: "Radio basics" });
    expect(link).toHaveAccessibleDescription("Page Mark as done Locked");
    expect(screen.getByText("Radio basics")).toHaveClass("text-fg-soft");
  });

  it("shows an overdue due label in the user's time zone", () => {
    // 02:00 UTC on 9 March is still 8 March in New York: two calendar days before 10 March.
    renderRow({ ...page, state: "overdue", due: new Date("2026-03-09T02:00:00Z") });
    expect(screen.getByRole("link")).toHaveAccessibleDescription(
      "Page Mark as done Overdue 2 days overdue",
    );
  });

  it("draws a header item as a heading, not a link", () => {
    renderWithStrings(
      <ol>
        <ItemRow
          item={{ id: "h", kind: "header", title: "Phraseology" }}
          timeZone={NY}
          now={now}
          headingLevel={4}
        />
      </ol>,
    );
    expect(screen.getByRole("heading", { level: 4, name: "Phraseology" })).toBeInTheDocument();
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByRole("listitem")).toHaveAttribute("data-kind", "header");
  });

  it.each<ItemRowItem>([
    { ...page, title: "«caller»", due: new Date(0) },
    { ...page, title: "«caller»", requirement: { kind: "watch", min_percent: 80 } },
    { id: "h", kind: "header", title: "«caller»" },
  ])("renders only catalog messages and the caller's content (%o)", (item) => {
    const { container } = renderRow(item, { marked: true });
    expect(unmarkedText(container)).toEqual([]);
  });
});
