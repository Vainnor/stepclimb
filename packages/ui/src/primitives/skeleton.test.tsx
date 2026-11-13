// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SKELETON_DELAY_MS, Skeleton } from "./skeleton";

describe("Skeleton", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("does not render before 300 ms (T12)", () => {
    const { container } = render(<Skeleton />);
    expect(SKELETON_DELAY_MS).toBe(300);
    act(() => {
      vi.advanceTimersByTime(299);
    });
    expect(container).toBeEmptyDOMElement();
  });

  it("renders at 300 ms, hidden from assistive technology, without animation", () => {
    const { container } = render(<Skeleton shape="circle" className="size-8" />);
    act(() => {
      vi.advanceTimersByTime(300);
    });
    const el = container.querySelector('[data-slot="skeleton"]');
    expect(el).toHaveAttribute("aria-hidden", "true");
    expect(el).toHaveClass("rounded-pill", "size-8", "bg-sunken");
    expect(el?.className).not.toMatch(/animate-/);
  });

  it("never renders when the content arrives first", () => {
    const { container, unmount } = render(<Skeleton />);
    act(() => {
      vi.advanceTimersByTime(200);
    });
    unmount();
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(container).toBeEmptyDOMElement();
    expect(vi.getTimerCount()).toBe(0);
  });
});
