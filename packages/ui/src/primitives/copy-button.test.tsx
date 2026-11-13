// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { CopyButton } from "./copy-button";

function stubClipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
}

describe("CopyButton", () => {
  beforeEach(() => stubMatchMedia(() => false));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
    Reflect.deleteProperty(navigator, "clipboard");
  });

  it("is a button named Copy from the catalog, with an empty status region", () => {
    renderWithStrings(<CopyButton value="ZDC-4417" />);
    const el = screen.getByRole("button", { name: "Copy" });
    expect(el).toHaveAttribute("data-slot", "copy-button");
    expect(el).toHaveAttribute("data-result", "idle");
    expect(screen.getByRole("status")).toHaveTextContent("");
    expect(screen.getByRole("status")).toHaveAttribute("data-slot", "copy-button-status");
  });

  it("copies the value and announces that it did (T12)", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async (_text: string) => undefined);
    stubClipboard(writeText);
    const onCopy = vi.fn();
    renderWithStrings(<CopyButton value="ZDC-4417" onCopy={onCopy} />);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledWith("ZDC-4417");
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    expect(screen.getByRole("button")).toHaveAttribute("data-result", "copied");
    expect(onCopy).toHaveBeenCalledWith("copied");
  });

  it("announces that the copy failed when the browser refuses (T12)", async () => {
    const user = userEvent.setup();
    stubClipboard(async () => {
      throw new DOMException("Document is not focused.", "NotAllowedError");
    });
    const onCopy = vi.fn();
    renderWithStrings(<CopyButton value="ZDC-4417" onCopy={onCopy} />);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Could not copy. Select the text and copy it yourself.",
    );
    expect(screen.getByRole("button")).toHaveAttribute("data-result", "failed");
    expect(onCopy).toHaveBeenCalledWith("failed");
  });

  it("announces failure where there is no Clipboard API", async () => {
    const user = userEvent.setup();
    // user-event puts a clipboard of its own on navigator; a page outside a secure context has none.
    Reflect.deleteProperty(navigator, "clipboard");
    expect(navigator.clipboard).toBeUndefined();
    renderWithStrings(<CopyButton value="ZDC-4417" />);
    await user.click(screen.getByRole("button", { name: "Copy" }));
    expect(screen.getByRole("status")).toHaveTextContent(/^Could not copy/);
  });

  it("returns to idle after resetAfterMs", async () => {
    vi.useFakeTimers();
    stubClipboard(async () => undefined);
    renderWithStrings(<CopyButton value="ZDC-4417" resetAfterMs={1000} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    });
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    await act(async () => {
      vi.advanceTimersByTime(999);
    });
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("status")).toHaveTextContent("");
    expect(screen.getByRole("button")).toHaveAttribute("data-result", "idle");
  });

  it("takes a label of its own", () => {
    renderWithStrings(<CopyButton value="x" label="Copy invite link" />);
    expect(screen.getByRole("button", { name: "Copy invite link" })).toBeInTheDocument();
  });

  it("says nothing that is not in the strings catalog (T2)", async () => {
    const user = userEvent.setup();
    stubClipboard(async () => undefined);
    const { container } = renderWithStrings(<CopyButton value="x" />, { marked: true });
    expect(screen.getByRole("button")).toHaveAccessibleName("«copyButton.label»");
    await user.click(screen.getByRole("button"));
    expect(screen.getByRole("status")).toHaveTextContent("«copyButton.copied»");
    expect(unmarkedText(container)).toEqual([]);
  });
});
