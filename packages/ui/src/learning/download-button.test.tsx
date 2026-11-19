// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { uiMessages } from "../strings/catalog";
import { StringsProvider } from "../strings/strings";
import { renderWithStrings, unmarkedText } from "../test/strings";
import {
  DownloadButton,
  type DownloadButtonProps,
  type DownloadProblem,
  type DownloadState,
} from "./download-button";

const NY = "America/New_York";

function setup(state: DownloadState, problem?: DownloadProblem) {
  const handlers = {
    onDownload: vi.fn(),
    onCancel: vi.fn(),
    onRemove: vi.fn(),
    onUpdate: vi.fn(),
  } satisfies Partial<DownloadButtonProps>;
  const view = renderWithStrings(
    <DownloadButton state={state} problem={problem} timeZone={NY} {...handlers} />,
  );
  return { ...view, handlers };
}

const buttonNames = () => screen.queryAllByRole("button").map((b) => b.textContent);

describe("DownloadButton", () => {
  it("offers a download with the package's size and the unencrypted-storage note", async () => {
    const { handlers } = setup({ kind: "none", sizeBytes: 12_400_000 });
    const download = screen.getByRole("button", { name: "Download for offline use" });
    expect(download).toHaveAccessibleDescription(
      "Takes 12.4 MB. The download stays in this browser and is not encrypted: anyone who can use this browser can read it.",
    );
    expect(buttonNames()).toEqual(["Download for offline use"]);
    await userEvent.click(download);
    expect(handlers.onDownload).toHaveBeenCalledOnce();
  });

  it("gives the note without a size when the caller does not know it yet", () => {
    setup({ kind: "none" });
    expect(screen.getByRole("button")).toHaveAccessibleDescription(
      "The download stays in this browser and is not encrypted: anyone who can use this browser can read it.",
    );
  });

  it("while downloading, shows the progress and offers only Cancel", async () => {
    const { handlers } = setup({
      kind: "downloading",
      receivedBytes: 3_000_000,
      totalBytes: 12_000_000,
    });
    const bar = screen.getByRole("progressbar", { name: "Downloading for offline use" });
    expect(bar).toHaveAttribute("aria-valuenow", "3000000");
    expect(bar).toHaveAttribute("aria-valuetext", "25%");
    expect(buttonNames()).toEqual(["Cancel download"]);
    await userEvent.click(screen.getByRole("button", { name: "Cancel download" }));
    expect(handlers.onCancel).toHaveBeenCalledOnce();
  });

  it("once downloaded, says when in the user's zone and how large, and offers Remove", async () => {
    // 02:00 UTC on 4 March is still 3 March in New York.
    const { handlers } = setup({
      kind: "downloaded",
      downloadedAt: new Date("2026-03-04T02:00:00Z"),
      sizeBytes: 1_500_000_000,
    });
    expect(screen.getByText(/^Downloaded/)).toHaveTextContent("Downloaded Mar 3, 2026 (1.5 GB)");
    expect(screen.queryByText("Update available")).toBeNull();
    expect(buttonNames()).toEqual(["Remove download"]);
    await userEvent.click(screen.getByRole("button", { name: "Remove download" }));
    expect(handlers.onRemove).toHaveBeenCalledOnce();
  });

  it("with an update available, says so and offers Update and Remove", async () => {
    const { handlers } = setup({
      kind: "update",
      downloadedAt: new Date("2026-03-03T15:00:00Z"),
      sizeBytes: 800_000,
    });
    expect(screen.getByText("Update available")).toBeInTheDocument();
    expect(screen.getByText(/^Downloaded/)).toHaveTextContent("Downloaded Mar 3, 2026 (800 kB)");
    expect(buttonNames()).toEqual(["Update download", "Remove download"]);
    await userEvent.click(screen.getByRole("button", { name: "Update download" }));
    expect(handlers.onUpdate).toHaveBeenCalledOnce();
    expect(handlers.onRemove).not.toHaveBeenCalled();
  });

  it.each<[DownloadProblem, string]>([
    [
      { kind: "tooLarge", sizeBytes: 620_000_000 },
      "This download is 620 MB, more than this app stores for one course.",
    ],
    [
      { kind: "noRoom", sizeBytes: 90_000_000 },
      "This download is 90 MB, more than the space this browser has left. Free up space, then try again.",
    ],
    [
      { kind: "failed" },
      "The download stopped before it finished, and what it had saved was removed. Try again.",
    ],
  ])("announces the problem %o", (problem, text) => {
    const { rerender } = setup({ kind: "none", sizeBytes: 1 });
    const alert = screen.getByRole("alert");
    expect(alert).toBeEmptyDOMElement();
    rerender(
      <DownloadButton
        state={{ kind: "none", sizeBytes: 1 }}
        problem={problem}
        timeZone={NY}
        onDownload={() => {}}
        onCancel={() => {}}
        onRemove={() => {}}
        onUpdate={() => {}}
      />,
    );
    expect(screen.getByRole("alert")).toBe(alert);
    expect(alert).toHaveTextContent(text);
  });

  it("names the course with the organization's term", () => {
    render(
      <StringsProvider
        catalogs={[uiMessages]}
        terms={{ course: { one: "training module", other: "training modules" } }}
        isDevelopment
      >
        <DownloadButton
          state={{ kind: "none" }}
          problem={{ kind: "tooLarge", sizeBytes: 620_000_000 }}
          timeZone={NY}
          onDownload={() => {}}
          onCancel={() => {}}
          onRemove={() => {}}
          onUpdate={() => {}}
        />
      </StringsProvider>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("for one training module.");
  });

  it.each<[DownloadState, DownloadProblem | undefined]>([
    [
      { kind: "none", sizeBytes: 1 },
      { kind: "tooLarge", sizeBytes: 1 },
    ],
    [{ kind: "none" }, { kind: "noRoom", sizeBytes: 1 }],
    [{ kind: "downloading", receivedBytes: 1, totalBytes: 2 }, { kind: "failed" }],
    [{ kind: "downloaded", downloadedAt: new Date(0), sizeBytes: 1 }, undefined],
    [{ kind: "update", downloadedAt: new Date(0), sizeBytes: 1 }, undefined],
  ])("renders only catalog messages (%o)", (state, problem) => {
    const { container } = renderWithStrings(
      <DownloadButton
        state={state}
        problem={problem}
        timeZone="UTC"
        onDownload={() => {}}
        onCancel={() => {}}
        onRemove={() => {}}
        onUpdate={() => {}}
      />,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
