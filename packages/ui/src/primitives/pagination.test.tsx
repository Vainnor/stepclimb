// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Pagination } from "./pagination";

describe("Pagination", () => {
  it("calls back with the next and previous cursors (T12)", async () => {
    const user = userEvent.setup();
    const onPageChange = vi.fn();
    renderWithStrings(
      <Pagination previousCursor="c-prev" nextCursor="c-next" onPageChange={onPageChange} />,
    );
    const nav = screen.getByRole("navigation", { name: "Pagination" });
    expect(nav).toHaveAttribute("data-slot", "pagination");
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Previous" }));
    expect(onPageChange.mock.calls).toEqual([
      ["c-next", "next"],
      ["c-prev", "previous"],
    ]);
  });

  it("disables a direction without a cursor", async () => {
    const user = userEvent.setup();
    const onPageChange = vi.fn();
    renderWithStrings(
      <Pagination previousCursor={null} nextCursor="c2" onPageChange={onPageChange} />,
    );
    const previous = screen.getByRole("button", { name: "Previous" });
    expect(previous).toBeDisabled();
    expect(previous).toHaveAttribute("data-slot", "pagination-previous");
    await user.click(previous);
    expect(onPageChange).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
  });

  it("shows the number of results only when given one", () => {
    const { rerender } = renderWithStrings(<Pagination nextCursor="c2" onPageChange={vi.fn()} />);
    expect(document.querySelector('[data-slot="pagination-count"]')).toBeNull();
    rerender(<Pagination nextCursor="c2" count={1} onPageChange={vi.fn()} />);
    expect(screen.getByText("1 result")).toHaveAttribute("data-slot", "pagination-count");
    rerender(<Pagination nextCursor="c2" count={1204} onPageChange={vi.fn()} />);
    expect(screen.getByText("1,204 results")).toBeInTheDocument();
  });
});

describe("Pagination after a page arrives", () => {
  it("announces the page and, on the last page, moves focus to Previous", async () => {
    const user = userEvent.setup();
    const onPageChange = vi.fn();
    const { rerender } = renderWithStrings(
      <Pagination previousCursor="c1" nextCursor="c3" onPageChange={onPageChange} />,
    );
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("");
    await user.click(screen.getByRole("button", { name: "Next" }));
    // The last page: no next cursor, so Next is disabled under the person's focus.
    rerender(<Pagination previousCursor="c2" nextCursor={null} onPageChange={onPageChange} />);
    expect(status).toHaveTextContent("Showing the next page");
    expect(screen.getByRole("button", { name: "Previous" })).toHaveFocus();
  });

  it("leaves focus alone when the pressed button stays enabled", async () => {
    const user = userEvent.setup();
    const { rerender } = renderWithStrings(
      <Pagination previousCursor="c1" nextCursor="c3" onPageChange={vi.fn()} />,
    );
    await user.click(screen.getByRole("button", { name: "Next" }));
    rerender(<Pagination previousCursor="c2" nextCursor="c4" onPageChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Next" })).toHaveFocus();
  });
});
