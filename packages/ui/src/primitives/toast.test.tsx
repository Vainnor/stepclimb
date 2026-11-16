// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { Button } from "./button";
import {
  createToastQueue,
  TOAST_MIN_TIMEOUT_MS,
  TOAST_TIMEOUT_MS,
  ToastRegion,
  toastTimeout,
} from "./toast";

// Every source file of the package but this test, read as text.
const sources = import.meta.glob<string>(["../**/*.{ts,tsx}", "!../**/toast.test.tsx"], {
  query: "?raw",
  import: "default",
  eager: true,
});

function region() {
  return screen.getByRole("region", { name: "Notifications" });
}

describe("Toast", () => {
  afterEach(() => vi.useRealTimers());

  it("renders nothing while the queue is empty, then the toast with its data-slots", () => {
    const queue = createToastQueue();
    renderWithStrings(<ToastRegion queue={queue} />);
    expect(screen.queryByRole("region")).toBeNull();
    act(() => {
      queue.add({ title: "Course saved", description: "Learners see it now.", tone: "success" });
    });
    expect(region()).toHaveAttribute("data-slot", "toast-region");
    const item = within(region()).getByRole("alertdialog", { name: "Course saved" });
    expect(item).toHaveAttribute("data-slot", "toast");
    expect(item).toHaveAttribute("data-tone", "success");
    expect(item).toHaveAccessibleDescription("Learners see it now.");
    expect(item.querySelector('[data-slot="toast-content"]')).toHaveAttribute("role", "alert");
    expect(item.querySelector('[data-slot="toast-title"]')).toHaveTextContent("Course saved");
    expect(item.querySelector('[data-slot="toast-close"]')).not.toBeNull();
  });

  it("announces without taking focus from what the person is using", async () => {
    const queue = createToastQueue();
    const user = userEvent.setup();
    renderWithStrings(
      <>
        <Button onPress={() => queue.add({ title: "Course saved" })}>Save course</Button>
        <ToastRegion queue={queue} />
      </>,
    );
    const button = screen.getByRole("button", { name: "Save course" });
    await user.click(button);
    expect(screen.getByRole("alert")).toHaveTextContent("Course saved");
    expect(button).toHaveFocus();
  });

  it("closes from its close button, named from the strings catalog", async () => {
    const queue = createToastQueue();
    const onClose = vi.fn();
    renderWithStrings(<ToastRegion queue={queue} />);
    act(() => {
      queue.add({ title: "Course saved" }, { onClose });
    });
    await userEvent.setup().click(screen.getByRole("button", { name: "Dismiss notification" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes a toast after its timeout, and closes one by key", () => {
    vi.useFakeTimers();
    const queue = createToastQueue();
    renderWithStrings(<ToastRegion queue={queue} />);
    let key = "";
    act(() => {
      queue.add({ title: "Course saved" });
      key = queue.add({ title: "Page published" });
    });
    act(() => queue.close(key));
    expect(screen.queryByRole("alertdialog", { name: "Page published" })).toBeNull();
    act(() => vi.advanceTimersByTime(TOAST_TIMEOUT_MS - 1));
    expect(screen.getByRole("alertdialog", { name: "Course saved" })).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(1));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("keeps an error toast until it is dismissed, whatever timeout is asked for", () => {
    vi.useFakeTimers();
    const queue = createToastQueue();
    renderWithStrings(<ToastRegion queue={queue} />);
    act(() => {
      queue.add({ title: "Could not sign out", tone: "error" }, { timeout: 5000 });
    });
    act(() => vi.advanceTimersByTime(60 * 60 * 1000));
    const item = screen.getByRole("alertdialog", { name: "Could not sign out" });
    expect(item).toHaveAttribute("data-tone", "error");
  });

  it("keeps a toast with an action until it is dismissed or the action is taken", async () => {
    vi.useFakeTimers();
    const queue = createToastQueue();
    const onAction = vi.fn();
    const onClose = vi.fn();
    renderWithStrings(<ToastRegion queue={queue} />);
    act(() => {
      queue.add(
        { title: "A new version is available", action: { label: "Reload", onAction } },
        { timeout: 5000, onClose },
      );
    });
    act(() => vi.advanceTimersByTime(60 * 60 * 1000));
    const item = screen.getByRole("alertdialog", { name: "A new version is available" });
    const action = within(item).getByRole("button", { name: "Reload" });
    expect(action).toHaveAttribute("data-slot", "toast-action");
    vi.useRealTimers();
    await userEvent.setup().click(action);
    expect(onAction).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("raises a timeout under the minimum, and keeps a longer one", () => {
    expect(toastTimeout({ title: "a" }, 1000)).toBe(TOAST_MIN_TIMEOUT_MS);
    expect(toastTimeout({ title: "a" }, 9000)).toBe(9000);
    expect(toastTimeout({ title: "a" })).toBe(TOAST_TIMEOUT_MS);
    expect(toastTimeout({ title: "a", tone: "error" }, 9000)).toBeUndefined();
    const action = { label: "Reload", onAction: () => {} };
    expect(toastTimeout({ title: "a", action }, 9000)).toBeUndefined();
  });

  it("pauses its timer while the region is hovered or has focus (WCAG 2.2.1)", () => {
    vi.useFakeTimers();
    const queue = createToastQueue();
    renderWithStrings(<ToastRegion queue={queue} />);
    act(() => {
      queue.add({ title: "Course saved" });
    });
    // React Aria's hover ignores emulated mouse events that follow touch; a pointer event is a mouse.
    fireEvent.pointerEnter(region(), { pointerType: "mouse" });
    act(() => vi.advanceTimersByTime(TOAST_TIMEOUT_MS * 3));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    fireEvent.pointerLeave(region(), { pointerType: "mouse" });
    act(() => {
      screen.getByRole("alertdialog").focus();
    });
    act(() => vi.advanceTimersByTime(TOAST_TIMEOUT_MS * 3));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    act(() => {
      screen.getByRole("alertdialog").blur();
    });
    act(() => vi.advanceTimersByTime(TOAST_TIMEOUT_MS));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("shows at most three at once, the newest first; the rest wait", () => {
    const queue = createToastQueue();
    renderWithStrings(<ToastRegion queue={queue} />);
    const keys: string[] = [];
    act(() => {
      for (const title of ["One", "Two", "Three", "Four"]) keys.push(queue.add({ title }));
    });
    const names = () =>
      screen
        .getAllByRole("alertdialog")
        .map((el) => el.querySelector('[data-slot="toast-title"]')?.textContent);
    expect(names()).toEqual(["Four", "Three", "Two"]);
    act(() => queue.close(keys[3] ?? ""));
    expect(names()).toEqual(["Three", "Two", "One"]);
  });

  it("refuses a queue it did not make", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    expect(() =>
      renderWithStrings(<ToastRegion queue={{ add: () => "", close: () => undefined }} />),
    ).toThrow(/createToastQueue/);
    vi.restoreAllMocks();
  });

  it("is the only file that imports React Aria's unstable toast exports (Q7)", () => {
    const importers = Object.entries(sources)
      .filter(([, text]) => /UNSTABLE_Toast/.test(text))
      .map(([file]) => file);
    expect(Object.keys(sources)).toContain("../patterns/app-shell.tsx");
    expect(importers).toEqual(["./toast.tsx"]);
  });
});
