// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Button, button } from "./button";

/** Announcer nodes that name themselves by an id no element in the document has. */
function danglingAnnouncements() {
  return [...document.querySelectorAll("[data-live-announcer] [aria-labelledby]")].filter((node) =>
    (node.getAttribute("aria-labelledby") ?? "")
      .split(/\s+/)
      .some((id) => id !== "" && document.getElementById(id) === null),
  );
}

/** Announcer nodes that name themselves by `el`'s id. */
function announcementsOf(el: HTMLElement) {
  return [...document.querySelectorAll("[data-live-announcer] [aria-labelledby]")].filter((node) =>
    (node.getAttribute("aria-labelledby") ?? "").split(/\s+/).includes(el.id),
  );
}

describe("Button", () => {
  it("renders a native button with its data-slot", () => {
    render(<Button>Start quiz</Button>);
    const el = screen.getByRole("button", { name: "Start quiz" });
    expect(el.tagName).toBe("BUTTON");
    expect(el).toHaveAttribute("data-slot", "button");
  });

  it("is secondary unless primary is asked for", () => {
    render(<Button>Preview</Button>);
    const el = screen.getByRole("button", { name: "Preview" });
    expect(el).toHaveClass("bg-raised", "border-line-strong");
    expect(el).not.toHaveClass("bg-brand");
  });

  it("applies the intent and size recipes", () => {
    render(
      <Button intent="primary" size="sm">
        Continue
      </Button>,
    );
    const el = screen.getByRole("button", { name: "Continue" });
    expect(el).toHaveClass("bg-brand", "text-on-brand", "h-(--sc-control-h-sm)");
  });

  it("presses on click, Enter and Space", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(<Button onPress={onPress}>Save</Button>);
    const el = screen.getByRole("button", { name: "Save" });

    await user.click(el);
    el.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");

    expect(onPress).toHaveBeenCalledTimes(3);
  });

  it("marks keyboard focus with data-focus-visible, which the ring is styled from", async () => {
    const user = userEvent.setup();
    render(<Button>Skip</Button>);
    await user.tab();
    expect(screen.getByRole("button", { name: "Skip" })).toHaveAttribute("data-focus-visible");
  });

  it("does not press when disabled", async () => {
    const user = userEvent.setup();
    const onPress = vi.fn();
    render(
      <Button isDisabled onPress={onPress}>
        Delete attempt
      </Button>,
    );
    const el = screen.getByRole("button", { name: "Delete attempt" });

    await user.click(el);

    expect(onPress).not.toHaveBeenCalled();
    expect(el).toBeDisabled();
    expect(el).toHaveAttribute("data-disabled");
  });

  it("lets className add layout without dropping the recipe", () => {
    expect(button({ className: "ms-auto" })).toContain("ms-auto");
    expect(button({ className: "ms-auto" })).toContain("bg-raised");
  });

  describe("pending", () => {
    it("keeps its label as the accessible name and shows a named spinner after 300 ms", () => {
      vi.useFakeTimers();
      renderWithStrings(<Button isPending>Submit quiz</Button>);
      const spinnerAt0 = screen.getByRole("progressbar", { name: "In progress" });
      expect(spinnerAt0).toHaveClass("opacity-0");
      expect(spinnerAt0).not.toHaveAttribute("data-shown");
      act(() => {
        vi.advanceTimersByTime(299);
      });
      expect(
        screen.getByRole("button").querySelector('[data-slot="button-label"]'),
      ).not.toHaveClass("opacity-0");
      act(() => {
        vi.advanceTimersByTime(1);
      });
      vi.useRealTimers();
      const el = screen.getByRole("button", { name: /^Submit quiz/ });
      expect(el).toHaveAttribute("data-pending", "true");
      expect(el).toHaveAttribute("aria-disabled", "true");
      expect(el.querySelector('[data-slot="button-label"]')).toHaveTextContent("Submit quiz");
      expect(el.querySelector('[data-slot="button-label"]')).toHaveClass("opacity-0");
      const spinner = screen.getByRole("progressbar", { name: "In progress" });
      expect(spinner).toHaveAttribute("data-slot", "button-pending");
      expect(spinner).toHaveAttribute("data-shown", "true");
      expect(spinner).not.toHaveClass("opacity-0");
      expect(el).toContainElement(spinner);
    });

    it("stays focusable and does not press", async () => {
      const user = userEvent.setup();
      const onPress = vi.fn();
      renderWithStrings(
        <Button isPending onPress={onPress}>
          Save
        </Button>,
      );
      await user.tab();
      const el = screen.getByRole("button", { name: /^Save/ });
      expect(el).toHaveFocus();
      await user.keyboard("{Enter}");
      await user.click(el);
      expect(onPress).not.toHaveBeenCalled();
    });

    it("draws no spinner and no label wrapper when not pending", () => {
      renderWithStrings(<Button>Save</Button>);
      expect(screen.queryByRole("progressbar")).toBeNull();
      expect(screen.getByRole("button").querySelector('[data-slot="button-label"]')).toBeNull();
    });

    it("names the spinner from the strings catalog (T2)", () => {
      const { container } = renderWithStrings(<Button isPending>«caller»</Button>, {
        marked: true,
      });
      expect(screen.getByRole("progressbar")).toHaveAccessibleName("«button.pending»");
      expect(unmarkedText(container)).toEqual([]);
    });

    it("announces its label when it turns pending while focused", async () => {
      const user = userEvent.setup();
      const { rerender } = renderWithStrings(<Button>Sign in</Button>);
      await user.tab();
      rerender(<Button isPending>Sign in</Button>);
      const el = screen.getByRole("button", { name: /^Sign in/ });
      expect(announcementsOf(el)).toHaveLength(1);
    });

    it("leaves no announcement pointing at it once it unmounts while pending", async () => {
      const user = userEvent.setup();
      const { rerender, unmount } = renderWithStrings(<Button>Sign in</Button>);
      await user.tab();
      rerender(<Button isPending>Sign in</Button>);
      unmount();
      expect(danglingAnnouncements()).toEqual([]);
    });

    it("leaves no announcement pointing at it once it unmounts after the wait ends", async () => {
      const user = userEvent.setup();
      const { rerender, unmount } = renderWithStrings(<Button aria-label="Verify">Go</Button>);
      await user.tab();
      rerender(
        <Button aria-label="Verify" isPending>
          Go
        </Button>,
      );
      rerender(<Button aria-label="Verify">Go</Button>);
      unmount();
      expect(danglingAnnouncements()).toEqual([]);
    });

    it("keeps another button's announcement when it unmounts", async () => {
      const user = userEvent.setup();
      const view = (savePending: boolean, signIn: "idle" | "pending" | "gone") => (
        <>
          <Button isPending={savePending}>Save</Button>
          {signIn === "gone" ? null : <Button isPending={signIn === "pending"}>Sign in</Button>}
        </>
      );
      const { rerender } = renderWithStrings(view(false, "idle"));
      await user.tab();
      rerender(view(true, "idle"));
      const save = screen.getByRole("button", { name: /^Save/ });
      await user.tab();
      rerender(view(true, "pending"));
      rerender(view(true, "gone"));
      expect(screen.getByRole("button", { name: /^Save/ })).toBe(save);
      expect(announcementsOf(save)).toHaveLength(1);
      expect(danglingAnnouncements()).toEqual([]);
    });
  });
});
