// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Announcement } from "./announcement";

// 03:00 UTC on 4 March is 22:00 on 3 March in New York.
const publishedAt = new Date("2026-03-04T03:00:00Z");
const html =
  '<p>The <a href="/c/1">ground course</a> opens next week.</p><ul><li>Bring a headset.</li></ul>';

describe("Announcement", () => {
  it("is an article named by its title, with the author and the date in the user's zone", () => {
    renderWithStrings(
      <Announcement
        title="New ground course"
        author="Pat Lee"
        publishedAt={publishedAt}
        timeZone="America/New_York"
        html={html}
      />,
    );
    const article = screen.getByRole("article", { name: "New ground course" });
    expect(article).toHaveAttribute("data-slot", "announcement");
    expect(within(article).getByRole("heading", { level: 2 })).toHaveTextContent(
      "New ground course",
    );
    expect(within(article).getByText("Posted by Pat Lee on Mar 3, 2026")).toHaveAttribute(
      "data-slot",
      "announcement-byline",
    );
  });

  it("reads the date in the zone it is given", () => {
    renderWithStrings(
      <Announcement title="T" author="Pat Lee" publishedAt={publishedAt} timeZone="UTC" html="" />,
    );
    expect(screen.getByText("Posted by Pat Lee on Mar 4, 2026")).toBeInTheDocument();
  });

  it("inserts the body HTML as the Prose's own children", () => {
    const { container } = renderWithStrings(
      <Announcement title="T" author="A" publishedAt={publishedAt} timeZone="UTC" html={html} />,
    );
    const prose = container.querySelector('[data-slot="prose"]');
    expect([...(prose?.children ?? [])].map((el) => el.tagName)).toEqual(["P", "UL"]);
    expect(screen.getByRole("link", { name: "ground course" })).toHaveAttribute("href", "/c/1");
  });

  it("takes the heading level the page needs", () => {
    renderWithStrings(
      <Announcement
        title="T"
        author="A"
        publishedAt={publishedAt}
        timeZone="UTC"
        html=""
        headingLevel={3}
      />,
    );
    expect(screen.getByRole("heading", { level: 3, name: "T" })).toBeInTheDocument();
  });

  it("renders only catalog messages and the caller's content", () => {
    const { container } = renderWithStrings(
      <Announcement
        title="«caller»"
        author="«caller»"
        publishedAt={publishedAt}
        timeZone="UTC"
        html="<p>«caller»</p>"
      />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
