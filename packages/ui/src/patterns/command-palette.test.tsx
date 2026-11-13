// SPDX-License-Identifier: AGPL-3.0-or-later
import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button } from "../primitives/button";
import { renderWithStrings } from "../test/strings";
import { type Command, CommandPalette, isPaletteShortcut } from "./command-palette";

const commands: Command[] = [
  { id: "courses", label: "Go to courses", keywords: ["catalog"] },
  { id: "grades", label: "Go to grades" },
  { id: "theme", label: "Switch to dark theme" },
  { id: "export", label: "Export report", isDisabled: true },
];

function palette() {
  return screen.getByRole("dialog", { name: "Commands" });
}

function search() {
  return screen.getByRole("searchbox", { name: "Search commands" });
}

function labels() {
  // React Aria wraps the empty state in a menuitem of its own; the commands carry the data-slot.
  return [...document.querySelectorAll('[data-slot="command-palette-item"]')].map(
    (item) => item.textContent,
  );
}

describe("CommandPalette", () => {
  it("opens on Ctrl+K with focus in its search box, and its data-slots", async () => {
    const user = userEvent.setup();
    renderWithStrings(<CommandPalette commands={commands} onAction={() => {}} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.keyboard("{Control>}k{/Control}");
    expect(palette()).toHaveAttribute("data-slot", "command-palette");
    expect(search()).toHaveFocus();
    expect(screen.getByRole("menu")).toHaveAttribute("data-slot", "command-palette-list");
    expect(screen.getAllByRole("menuitem")[0]).toHaveAttribute("data-slot", "command-palette-item");
    expect(palette().closest('[data-slot="command-palette-overlay"]')).toHaveClass(
      "h-(--visual-viewport-height)",
    );
  });

  it("opens on ⌘K, and the shortcut closes it again", async () => {
    const user = userEvent.setup();
    renderWithStrings(<CommandPalette commands={commands} onAction={() => {}} />);
    await user.keyboard("{Meta>}k{/Meta}");
    expect(palette()).toBeInTheDocument();
    await user.keyboard("{Meta>}k{/Meta}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("leaves the shortcut to a handler that already took it", () => {
    renderWithStrings(
      <>
        {/* biome-ignore lint/a11y/noStaticElementInteractions: an editor that claims Ctrl+K */}
        <div
          data-testid="editor"
          onKeyDown={(event) => {
            if (event.key === "k") event.preventDefault();
          }}
        />
        <CommandPalette commands={commands} onAction={() => {}} />
      </>,
    );
    fireEvent.keyDown(screen.getByTestId("editor"), { key: "k", ctrlKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("recognizes only Ctrl+K and ⌘K", () => {
    const key = (init: KeyboardEventInit) => isPaletteShortcut(new KeyboardEvent("keydown", init));
    expect(key({ key: "k", ctrlKey: true })).toBe(true);
    expect(key({ key: "K", metaKey: true })).toBe(true);
    expect(key({ key: "k" })).toBe(false);
    expect(key({ key: "k", ctrlKey: true, shiftKey: true })).toBe(false);
    expect(key({ key: "k", ctrlKey: true, altKey: true })).toBe(false);
    expect(key({ key: "j", ctrlKey: true })).toBe(false);
  });

  it("filters by label and keyword, ignoring case, and says when nothing matches", async () => {
    const user = userEvent.setup();
    renderWithStrings(<CommandPalette defaultOpen commands={commands} onAction={() => {}} />);
    await user.type(search(), "GO TO");
    expect(labels()).toEqual(["Go to courses", "Go to grades"]);
    await user.clear(search());
    await user.type(search(), "catalog");
    expect(labels()).toEqual(["Go to courses"]);
    await user.type(search(), "zzz");
    expect(labels()).toEqual([]);
    expect(screen.getByText("No commands match your search.")).toHaveAttribute(
      "data-slot",
      "command-palette-empty",
    );
  });

  it("moves with the arrow keys while focus stays in the search box, and Enter runs", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    const onOpenChange = vi.fn();
    renderWithStrings(
      <CommandPalette
        defaultOpen
        commands={commands}
        onAction={onAction}
        onOpenChange={onOpenChange}
      />,
    );
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{ArrowDown}");
    expect(search()).toHaveFocus();
    const active = search().getAttribute("aria-activedescendant");
    expect(active && document.getElementById(active)).toHaveTextContent("Go to grades");
    await user.keyboard("{Enter}");
    expect(onAction).toHaveBeenCalledWith("grades");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("lets Tab reach the list's scrolling box, so a keyboard can scroll it", async () => {
    const user = userEvent.setup();
    renderWithStrings(<CommandPalette defaultOpen commands={commands} onAction={() => {}} />);
    await user.tab();
    expect(screen.getByRole("menu").parentElement).toHaveFocus();
    expect(screen.getByRole("menu").parentElement).toHaveAttribute(
      "data-slot",
      "command-palette-scroll",
    );
  });

  it("does not run a disabled command", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    renderWithStrings(<CommandPalette defaultOpen commands={commands} onAction={onAction} />);
    await user.click(screen.getByRole("menuitem", { name: "Export report" }));
    expect(onAction).not.toHaveBeenCalled();
  });

  it("clears the search on Escape, then closes and returns focus", async () => {
    const user = userEvent.setup();
    renderWithStrings(
      <>
        <Button>Before</Button>
        <CommandPalette commands={commands} onAction={() => {}} />
      </>,
    );
    const before = screen.getByRole("button", { name: "Before" });
    await user.click(before);
    await user.keyboard("{Control>}k{/Control}");
    await user.type(search(), "grades");
    await user.keyboard("{Escape}");
    expect(search()).toHaveValue("");
    expect(palette()).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(before).toHaveFocus());
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    renderWithStrings(
      <CommandPalette isOpen commands={commands} onAction={() => {}} onOpenChange={onOpenChange} />,
    );
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(palette()).toBeInTheDocument();
  });
});
