// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { ProgressRail, type ProgressRailItem } from "./progress-rail";

const NY = "America/New_York";
const now = new Date("2026-03-10T14:00:00Z");

const items: ProgressRailItem[] = [
  { id: "a", title: "Radio basics", href: "/c/1/m/1/i/a", state: "complete" },
  { id: "b", title: "Phraseology", href: "/c/1/m/1/i/b", state: "progress" },
  { id: "c", title: "Readback drill", href: "/c/1/m/1/i/c", state: "available" },
  {
    id: "d",
    title: "Clearance delivery",
    href: "/c/1/m/1/i/d",
    state: "overdue",
    due: new Date("2026-03-07T14:00:00Z"),
  },
  { id: "e", title: "Review", href: "/c/1/m/1/i/e", state: "review" },
  {
    id: "f",
    title: "Checkout",
    href: "/c/1/m/1/i/f",
    state: "locked",
    lockReasons: [
      { kind: "sequential", itemTitle: "Clearance delivery" },
      { kind: "unlock_at", at: new Date("2026-03-20T15:00:00Z") },
    ],
  },
];

function rail(props: Partial<Parameters<typeof ProgressRail>[0]> = {}) {
  return (
    <ProgressRail
      aria-label="Ground control"
      items={items}
      currentId="c"
      timeZone={NY}
      now={now}
      {...props}
    />
  );
}

const links = () => screen.getAllByRole("link");

describe("ProgressRail", () => {
  it("is an ordered list of links named by the module, in item order", () => {
    renderWithStrings(rail());
    const list = screen.getByRole("list", { name: "Ground control" });
    expect(list.tagName).toBe("OL");
    expect(links().map((l) => l.getAttribute("href"))).toEqual(items.map((i) => i.href));
  });

  it("names each node with its title and learning state (T2)", () => {
    renderWithStrings(rail());
    expect(links().map((l) => l.getAttribute("aria-label"))).toEqual([
      "Radio basics, Complete",
      "Phraseology, In progress",
      "Readback drill, Available",
      "Clearance delivery, 3 days overdue",
      "Review, Needs review",
      "Checkout, Locked",
    ]);
    // The visible title starts the name (WCAG 2.5.3).
    for (const link of links()) {
      const title = link.querySelector('[data-slot="progress-rail-title"]')?.textContent ?? "";
      expect(link.getAttribute("aria-label")?.startsWith(`${title}, `)).toBe(true);
    }
  });

  it("draws each state with its own icon and token (T2)", () => {
    renderWithStrings(rail({ currentId: undefined }));
    const drawings = links().map((link) => {
      const icon = link.querySelector('[data-slot="progress-rail-node"] svg');
      expect(icon).toHaveClass(`text-state-${link.getAttribute("data-state")}`);
      return icon?.innerHTML;
    });
    expect(new Set(drawings).size).toBe(items.length);
  });

  it("marks the current item as the page, ringed, with a dot while available", () => {
    renderWithStrings(rail());
    const current = screen.getByRole("link", { current: "page" });
    expect(current).toHaveAttribute("href", "/c/1/m/1/i/c");
    expect(links().filter((l) => l.getAttribute("aria-current") === "page")).toHaveLength(1);
    const node = current.querySelector('[data-slot="progress-rail-node"]');
    expect(node).toHaveClass("border-brand");
    expect(node?.querySelector("svg")).toHaveClass("text-brand");
  });

  it("rings an overdue node in the overdue token and shows its due label", () => {
    renderWithStrings(rail());
    const overdue = screen.getByRole("link", { name: /Clearance delivery/ });
    expect(overdue.querySelector('[data-slot="progress-rail-node"]')).toHaveClass(
      "border-state-overdue",
    );
    expect(overdue.querySelector('[data-slot="due-badge"]')).toHaveTextContent("3 days overdue");
    // The due label is part of the name, so the name holds all the text the link draws.
    expect(overdue).toHaveAccessibleName("Clearance delivery, 3 days overdue");
    expect(overdue).not.toHaveAttribute("aria-describedby");
  });

  it("keeps the overdue ring on a current overdue node and rings it in the brand outside", () => {
    renderWithStrings(rail({ currentId: "d" }));
    const node = screen
      .getByRole("link", { current: "page" })
      .querySelector('[data-slot="progress-rail-node"]');
    expect(node).toHaveClass("border-state-overdue", "outline-2", "outline-brand");
    expect(node).not.toHaveClass("border-brand");
  });

  it("rings a current node in every other state in the brand", () => {
    for (const id of ["a", "b", "e", "f"]) {
      const { unmount } = renderWithStrings(rail({ currentId: id }));
      const node = screen
        .getByRole("link", { current: "page" })
        .querySelector('[data-slot="progress-rail-node"]');
      expect(node).toHaveClass("border-2", "border-brand");
      unmount();
    }
  });

  it("keeps a locked node focusable and announces what unlocks it (T4)", async () => {
    renderWithStrings(rail());
    const locked = screen.getByRole("link", { name: /Checkout/ });
    expect(locked).toHaveAttribute("href", "/c/1/m/1/i/f");
    // Two reasons, joined as the locale lists units.
    expect(locked).toHaveAccessibleDescription(
      "Unlocks after Clearance delivery, Opens on Mar 20, 2026, 11:00 AM EDT",
    );
    // Only a locked node has a description.
    expect(screen.getByRole("link", { name: /Radio basics/ })).not.toHaveAttribute(
      "aria-describedby",
    );
    // React Aria opens the tooltip on keyboard focus.
    await userEvent.tab();
    await userEvent.keyboard("{End}");
    expect(locked).toHaveFocus();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Unlocks after Clearance deliveryOpens on Mar 20, 2026, 11:00 AM EDT",
    );
  });

  it("lists every prerequisite and says when a module is locked", () => {
    renderWithStrings(
      rail({
        items: [
          {
            id: "x",
            title: "Tower",
            href: "/x",
            state: "locked",
            lockReasons: [{ kind: "prerequisites", moduleTitles: ["Ground", "Delivery"] }],
          },
          {
            id: "y",
            title: "Approach",
            href: "/y",
            state: "locked",
            lockReasons: [{ kind: "module_locked" }],
          },
        ],
      }),
    );
    expect(screen.getByRole("link", { name: /Tower/ })).toHaveAccessibleDescription(
      "Unlocks after Ground and Delivery",
    );
    expect(screen.getByRole("link", { name: /Approach/ })).toHaveAccessibleDescription(
      "Unlocks when its module unlocks",
    );
  });

  it("moves focus with Up, Down, Home and End, and stops at the ends (T4)", async () => {
    renderWithStrings(rail());
    await userEvent.tab();
    expect(links()[0]).toHaveFocus();
    await userEvent.keyboard("{ArrowUp}");
    expect(links()[0]).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}{ArrowDown}");
    expect(links()[2]).toHaveFocus();
    await userEvent.keyboard("{End}");
    expect(links()[5]).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(links()[5]).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(links()[0]).toHaveFocus();
  });

  it("fills the segment after each complete item, and none after the last", () => {
    const { container } = renderWithStrings(rail());
    const steps = [...container.querySelectorAll('[data-slot="progress-rail-step"]')];
    const fills = steps.map((s) => s.querySelector('[data-slot="progress-rail-fill"]'));
    expect(fills.at(-1)).toBeNull();
    expect(fills[0]).toHaveClass("scale-y-100");
    for (const fill of fills.slice(1, -1)) expect(fill).toHaveClass("scale-y-0");
  });

  it("fills a segment over the slow motion token when its item completes (T4)", () => {
    const { container, rerender } = renderWithStrings(rail());
    const fill = () =>
      container.querySelectorAll('[data-slot="progress-rail-fill"]')[1] as HTMLElement;
    const before = fill();
    expect(before).toHaveClass("scale-y-0", "transition-[scale]", "duration-slow");
    const done = items.map((i) => (i.id === "b" ? { ...i, state: "complete" as const } : i));
    rerender(rail({ items: done }));
    // The same element changes class, so the browser runs its transition.
    expect(fill()).toBe(before);
    expect(fill()).toHaveClass("scale-y-100", "transition-[scale]", "duration-slow");
  });

  it("fills the whole rail once when the course completes, not when drawn complete (T4)", () => {
    const complete = items.map(({ id, title, href }) => ({
      id,
      title,
      href,
      state: "complete" as const,
    }));
    const { container, rerender } = renderWithStrings(
      rail({ items: complete, courseComplete: true }),
    );
    const list = () => container.querySelector('[data-slot="progress-rail"]');
    expect(list()).not.toHaveAttribute("data-celebrate");

    const second = renderWithStrings(rail({ courseComplete: false }));
    const secondList = () => second.container.querySelector('[data-slot="progress-rail"]');
    expect(secondList()).not.toHaveAttribute("data-celebrate");
    second.rerender(rail({ items: complete, courseComplete: true }));
    expect(secondList()).toHaveAttribute("data-celebrate");
    expect((secondList() as HTMLElement).style.getPropertyValue("--rail-segments")).toBe("5");
    const steps = second.container.querySelectorAll<HTMLElement>(
      '[data-slot="progress-rail-step"]',
    );
    expect([...steps].map((s) => s.style.getPropertyValue("--rail-step"))).toEqual([
      "0",
      "1",
      "2",
      "3",
      "4",
      "5",
    ]);
    // The fill's animation runs when the list carries data-celebrate, on the expo ease-out token.
    expect(steps[0]?.querySelector('[data-slot="progress-rail-fill"]')?.className).toContain(
      "group-data-[celebrate]/rail:animate-[sc-rail-fill_calc(var(--sc-motion-slow)/var(--rail-segments))_var(--sc-ease-out-expo)_both]",
    );
    rerender(rail({ items: complete, courseComplete: true }));
    expect(list()).not.toHaveAttribute("data-celebrate");
  });

  it("fills every segment once the course is complete, even after items not complete", () => {
    const { container } = renderWithStrings(rail({ courseComplete: true }));
    const fills = [...container.querySelectorAll('[data-slot="progress-rail-fill"]')];
    expect(fills).toHaveLength(items.length - 1);
    for (const fill of fills) expect(fill).toHaveClass("scale-y-100");
  });

  it("shows the empty state for a module without items", () => {
    renderWithStrings(rail({ items: [], emptyState: "Nothing here yet" }));
    expect(screen.queryByRole("list")).toBeNull();
    expect(screen.getByText("Nothing here yet")).toHaveAttribute(
      "data-slot",
      "progress-rail-empty",
    );
  });

  it("renders only catalog messages and the caller's content", () => {
    const { container } = renderWithStrings(
      rail({ "aria-label": "«caller»", items: items.map((i) => ({ ...i, title: "«caller»" })) }),
      {
        marked: true,
      },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
