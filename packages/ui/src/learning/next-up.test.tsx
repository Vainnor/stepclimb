// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { NextUp, type NextUpStatus } from "./next-up";

const NY = "America/New_York";
const next: NextUpStatus = {
  kind: "next",
  itemTitle: "Taxi instructions",
  moduleTitle: "Ground control",
  href: "/c/1/m/2/i/3",
  started: false,
};

describe("NextUp", () => {
  it("names its section Next up and offers one Continue link to the item", () => {
    renderWithStrings(<NextUp status={next} timeZone={NY} />);
    const panel = screen.getByRole("region", { name: "Next up" });
    expect(panel).toHaveAttribute("data-status", "next");
    const links = within(panel).getAllByRole("link");
    expect(links).toHaveLength(1);
    // The name starts with the visible label (axe label-in-name) and says which item it opens.
    expect(links[0]).toHaveAccessibleName("Continue: Taxi instructions");
    expect(links[0]).toHaveTextContent(/^Continue$/);
    expect(links[0]).not.toHaveAttribute("aria-describedby");
    expect(links[0]).toHaveAttribute("href", "/c/1/m/2/i/3");
    expect(links[0]).toHaveClass("bg-brand");
    expect(within(panel).getByText("Ground control")).toHaveAttribute(
      "data-slot",
      "next-up-module",
    );
  });

  it("says whether the learner has started the item", () => {
    const { rerender } = renderWithStrings(<NextUp status={next} timeZone={NY} />);
    expect(screen.getByText("Available")).toBeInTheDocument();
    rerender(<NextUp status={{ ...next, started: true }} timeZone={NY} />);
    expect(screen.getByText("In progress")).toBeInTheDocument();
  });

  it("while waiting, says when the course opens further in the user's zone, with no action", () => {
    renderWithStrings(
      <NextUp
        status={{ kind: "waiting", until: new Date("2026-03-03T15:00:00Z") }}
        timeZone={NY}
      />,
    );
    const panel = screen.getByRole("region", { name: "Next up" });
    expect(panel).toHaveAttribute("data-status", "waiting");
    expect(within(panel).queryByRole("link")).toBeNull();
    expect(within(panel).getByText(/^Opens on/)).toHaveTextContent(
      "LockedOpens on Mar 3, 2026, 10:00 AM EST",
    );
  });

  it("once complete, says when in the user's zone, with no action", () => {
    // 02:00 UTC on 4 March is still 3 March in New York.
    renderWithStrings(
      <NextUp
        status={{ kind: "complete", completedAt: new Date("2026-03-04T02:00:00Z") }}
        timeZone={NY}
      />,
    );
    const panel = screen.getByRole("region", { name: "Next up" });
    expect(within(panel).queryByRole("link")).toBeNull();
    expect(within(panel).getByText(/Completed/)).toHaveTextContent("CompleteCompleted Mar 3, 2026");
  });

  it("draws the course progress when given, named with the organization's term", () => {
    render(
      <StringsProvider
        catalogs={[uiMessages]}
        terms={{ course: { one: "training module", other: "training modules" } }}
        isDevelopment
      >
        <NextUp status={next} timeZone={NY} progress={{ completed: 3, required: 5 }} />
      </StringsProvider>,
    );
    const bar = screen.getByRole("progressbar", { name: "Training module progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "3");
    expect(bar).toHaveAttribute("aria-valuemax", "5");
    expect(bar).toHaveAttribute("aria-valuetext", "60%");
  });

  it("shows the course title only when given, and no bar without progress", () => {
    const { rerender } = renderWithStrings(<NextUp status={next} timeZone={NY} />);
    expect(document.querySelector('[data-slot="next-up-course"]')).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
    rerender(<NextUp status={next} timeZone={NY} courseTitle="S1 Ground theory" />);
    expect(screen.getByText("S1 Ground theory")).toHaveAttribute("data-slot", "next-up-course");
  });

  it("takes the heading level the page needs", () => {
    renderWithStrings(<NextUp status={next} timeZone={NY} headingLevel={3} />);
    expect(screen.getByRole("heading", { level: 3, name: "Next up" })).toBeInTheDocument();
  });

  it.each<NextUpStatus>([
    { ...next, itemTitle: "«caller»", moduleTitle: "«caller»" },
    { kind: "waiting", until: new Date(0) },
    { kind: "complete", completedAt: new Date(0) },
  ])("renders only catalog messages and the caller's content (%o)", (status) => {
    const { container } = renderWithStrings(
      <NextUp
        status={status}
        timeZone="UTC"
        courseTitle="«caller»"
        progress={{ completed: 1, required: 2 }}
      />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
