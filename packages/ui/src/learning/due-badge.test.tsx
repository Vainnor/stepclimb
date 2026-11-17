// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { DueBadge, dueDays } from "./due-badge";

const NY = "America/New_York";
const at = (iso: string) => new Date(iso);

describe("dueDays", () => {
  it("counts calendar days in the user's time zone, not 24-hour periods", () => {
    // 23:00 New York on 10 March is 03:00 UTC on 11 March; due 09:00 New York on 11 March.
    const now = at("2026-03-11T03:00:00Z");
    const due = at("2026-03-11T13:00:00Z");
    expect(dueDays(due, now, NY)).toEqual({ overdue: false, days: 1 });
    expect(dueDays(due, now, "UTC")).toEqual({ overdue: false, days: 0 });
  });

  it("is not overdue at the due instant and overdue a millisecond after", () => {
    const due = at("2026-03-11T13:00:00Z");
    expect(dueDays(due, due, NY)).toEqual({ overdue: false, days: 0 });
    expect(dueDays(due, at("2026-03-11T13:00:00.001Z"), NY)).toEqual({ overdue: true, days: 0 });
  });

  it("counts days late once past", () => {
    const due = at("2026-03-05T13:00:00Z");
    expect(dueDays(due, at("2026-03-08T15:00:00Z"), NY)).toEqual({ overdue: true, days: 3 });
  });

  it("counts across the daylight-saving change by the calendar", () => {
    // 8 March 2026 is a 23-hour day in New York.
    const due = at("2026-03-09T13:00:00Z");
    expect(dueDays(due, at("2026-03-07T14:00:00Z"), NY)).toEqual({ overdue: false, days: 2 });
  });
});

describe("DueBadge", () => {
  const now = at("2026-03-10T14:00:00Z");
  const cases: [string, string, boolean][] = [
    ["2026-03-13T14:00:00Z", "Due in 3 days", false],
    ["2026-03-11T14:00:00Z", "Due tomorrow", false],
    ["2026-03-10T20:00:00Z", "Due today", false],
    ["2026-03-10T13:00:00Z", "Overdue", true],
    ["2026-03-09T14:00:00Z", "1 day overdue", true],
    ["2026-03-07T14:00:00Z", "3 days overdue", true],
  ];

  it.each(cases)("due %s shows %s", (due, label, overdue) => {
    const { container } = renderWithStrings(<DueBadge due={at(due)} now={now} timeZone={NY} />);
    const badge = container.querySelector('[data-slot="due-badge"]');
    expect(screen.getByText(label)).toHaveAttribute("dateTime", at(due).toISOString());
    expect(badge?.hasAttribute("data-overdue")).toBe(overdue);
    expect(container.querySelector('[data-slot="icon"]')).toHaveClass(
      overdue ? "text-state-overdue" : "text-fg-sub",
    );
  });

  it("draws overdue with a different icon, so color is not the only signal", () => {
    const { container: late } = renderWithStrings(
      <DueBadge due={at("2026-03-01T00:00:00Z")} now={now} timeZone={NY} />,
    );
    const { container: soon } = renderWithStrings(
      <DueBadge due={at("2026-03-20T00:00:00Z")} now={now} timeZone={NY} />,
    );
    expect(late.querySelector("svg")?.innerHTML).not.toEqual(soon.querySelector("svg")?.innerHTML);
  });

  it("renders only catalog messages", () => {
    const { container } = renderWithStrings(
      <DueBadge due={at("2026-03-01T00:00:00Z")} now={now} timeZone={NY} />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
