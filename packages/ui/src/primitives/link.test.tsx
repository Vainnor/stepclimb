// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { isExternalHref, Link } from "./link";

const origin = "https://learn.example.test";

describe("isExternalHref", () => {
  it.each([
    ["https://other.example.test/page", true],
    ["//other.example.test/page", true],
    ["http://learn.example.test/page", true],
    ["https://learn.example.test/courses", false],
    ["/courses/1", false],
    ["courses/1", false],
    ["#section", false],
    ["mailto:help@example.test", false],
    ["tel:+15555550100", false],
    [undefined, false],
  ])("%s is external: %s", (href, expected) => {
    expect(isExternalHref(href, origin)).toBe(expected);
  });
});

describe("Link", () => {
  it("renders an anchor with its data-slot", () => {
    renderWithStrings(<Link href="/courses">Courses</Link>);
    const el = screen.getByRole("link", { name: "Courses" });
    expect(el.tagName).toBe("A");
    expect(el).toHaveAttribute("data-slot", "link");
    expect(el).toHaveAttribute("href", "/courses");
  });

  it("navigates through the app's RouterProvider on click and Enter", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn();
    renderWithStrings(
      <RouterProvider navigate={navigate}>
        <Link href="/courses/7">Ground control</Link>
      </RouterProvider>,
    );
    const el = screen.getByRole("link", { name: "Ground control" });

    await user.click(el);
    el.focus();
    await user.keyboard("{Enter}");

    expect(navigate).toHaveBeenCalledTimes(2);
    expect(navigate).toHaveBeenCalledWith("/courses/7", undefined);
  });

  it("does not hand another site to the router, and tells screen readers it leaves", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn();
    renderWithStrings(
      <RouterProvider navigate={navigate}>
        <Link href="https://other.example.test/charts">Charts</Link>
      </RouterProvider>,
    );
    // jsdom's name computation drops the hidden span's leading space; a browser keeps it.
    const el = screen.getByRole("link", { name: /^Charts ?\(external site\)$/ });
    el.addEventListener("click", (e) => e.preventDefault());

    await user.click(el);

    expect(navigate).not.toHaveBeenCalled();
    expect(el.querySelector('[data-slot="link-external"]')).toHaveTextContent(
      /^ \(external site\)$/,
      {
        normalizeWhitespace: false,
      },
    );
  });

  it("says nothing extra for an in-app link", () => {
    renderWithStrings(<Link href="/grades">Grades</Link>);
    const el = screen.getByRole("link", { name: "Grades" });
    expect(el.querySelector('[data-slot="link-external"]')).toBeNull();
  });

  it("marks keyboard focus with data-focus-visible", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Link href="/grades">Grades</Link>);
    await user.tab();
    expect(screen.getByRole("link", { name: "Grades" })).toHaveAttribute("data-focus-visible");
  });

  it("does not navigate when disabled", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn();
    renderWithStrings(
      <RouterProvider navigate={navigate}>
        <Link href="/grades" isDisabled>
          Grades
        </Link>
      </RouterProvider>,
    );
    const el = screen.getByRole("link", { name: "Grades" });

    await user.click(el);

    expect(navigate).not.toHaveBeenCalled();
    expect(el).toHaveAttribute("aria-disabled", "true");
    expect(el).toHaveAttribute("data-disabled");
  });

  it("takes its screen-reader text from the strings catalog (T2)", () => {
    const { container } = renderWithStrings(
      <Link href="https://other.example.test/">«caller»</Link>,
      { marked: true },
    );
    expect(screen.getByRole("link")).toHaveAccessibleName(/^«caller» ?«link\.external»$/);
    expect(unmarkedText(container)).toEqual([]);
  });

  it("lets className add layout without dropping the recipe", () => {
    renderWithStrings(
      <Link href="/grades" className="ms-auto">
        Grades
      </Link>,
    );
    expect(screen.getByRole("link", { name: "Grades" })).toHaveClass("ms-auto", "text-brand-fg");
  });
});
