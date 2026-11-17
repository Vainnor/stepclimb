// SPDX-License-Identifier: AGPL-3.0-or-later
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { CourseHeader, type CourseStatus } from "./course-header";

describe("CourseHeader", () => {
  it("shows the title as the page's heading and the code", () => {
    renderWithStrings(<CourseHeader title="S1 Ground theory" code="S1-GND" />);
    expect(screen.getByRole("heading", { level: 1, name: "S1 Ground theory" })).toBeInTheDocument();
    expect(screen.getByText("S1-GND")).toHaveAttribute("data-slot", "course-header-code");
  });

  it("draws the cover as a decorative image", () => {
    const { container } = renderWithStrings(
      <CourseHeader title="S1 Ground theory" code="S1-GND" coverUrl="/files/cover.png" />,
    );
    const cover = container.querySelector("img");
    expect(cover).toHaveAttribute("src", "/files/cover.png");
    expect(cover).toHaveAttribute("alt", "");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("leaves out a cover that fails to load, and tries a new URL", () => {
    const { container, rerender } = renderWithStrings(
      <CourseHeader title="S1 Ground theory" code="S1-GND" coverUrl="/files/expired.png" />,
    );
    fireEvent.error(container.querySelector("img") as HTMLImageElement);
    expect(container.querySelector("img")).toBeNull();
    rerender(<CourseHeader title="S1 Ground theory" code="S1-GND" coverUrl="/files/fresh.png" />);
    expect(container.querySelector("img")).toHaveAttribute("src", "/files/fresh.png");
  });

  it("draws no cover without a URL", () => {
    const { container } = renderWithStrings(
      <CourseHeader title="S1 Ground theory" code="S1-GND" coverUrl={null} />,
    );
    expect(container.querySelector("img")).toBeNull();
  });

  it("draws the learner's progress, named with the organization's term", () => {
    render(
      <StringsProvider
        catalogs={[uiMessages]}
        terms={{ course: { one: "training module", other: "training modules" } }}
        isDevelopment
      >
        <CourseHeader title="S1" code="S1" progress={{ completed: 2, required: 8 }} />
      </StringsProvider>,
    );
    const bar = screen.getByRole("progressbar", { name: "Training module progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "2");
    expect(bar).toHaveAttribute("aria-valuemax", "8");
  });

  it("shows no progress bar and no status unless given", () => {
    const { container } = renderWithStrings(<CourseHeader title="S1" code="S1" />);
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(container.querySelector('[data-slot="course-header-status"]')).toBeNull();
  });

  it.each<[CourseStatus, string]>([
    ["draft", "Draft"],
    ["published", "Published"],
    ["archived", "Archived"],
  ])("shows staff the %s status in words", (status, words) => {
    renderWithStrings(<CourseHeader title="S1" code="S1" status={status} />);
    expect(screen.getByText(words)).toHaveAttribute("data-status", status);
  });

  it("takes the heading level the page needs", () => {
    renderWithStrings(<CourseHeader title="S1" code="S1" headingLevel={2} />);
    expect(screen.getByRole("heading", { level: 2, name: "S1" })).toBeInTheDocument();
  });

  it("renders only catalog messages and the caller's content", () => {
    const { container } = renderWithStrings(
      <CourseHeader
        title="«caller»"
        code="«caller»"
        coverUrl="/c.png"
        status="draft"
        progress={{ completed: 1, required: 2 }}
      />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
