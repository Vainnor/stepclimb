// SPDX-License-Identifier: AGPL-3.0-or-later
import { FolderIcon } from "@phosphor-icons/react";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Key } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Tree, TreeItem } from "./tree";

function Accounts(props: {
  expandedKeys?: Iterable<Key>;
  defaultExpandedKeys?: Iterable<Key>;
  onExpandedChange?: (keys: Set<Key>) => void;
  onAction?: (key: Key) => void;
}) {
  return (
    <Tree aria-label="Accounts" {...props}>
      <TreeItem id="root" title="Example Air Traffic" icon={FolderIcon}>
        <TreeItem id="tower" title="Tower" icon={FolderIcon}>
          <TreeItem id="ground" title="Ground" />
        </TreeItem>
        <TreeItem id="enroute" title="Enroute" />
      </TreeItem>
    </Tree>
  );
}

const row = (name: string) => screen.getByRole("row", { name: new RegExp(`^${name}`) });
const rows = () => screen.queryAllByRole("row").map((r) => r.textContent);

describe("Tree", () => {
  it("is a named treegrid with its data-slots and levels", () => {
    renderWithStrings(<Accounts defaultExpandedKeys={["root"]} />);
    const tree = screen.getByRole("treegrid", { name: "Accounts" });
    expect(tree).toHaveAttribute("data-slot", "tree");
    expect(row("Tower")).toHaveAttribute("data-slot", "tree-item");
    expect(row("Tower")).toHaveAttribute("aria-level", "2");
    expect(row("Example Air Traffic")).toHaveAttribute("aria-level", "1");
  });

  it("shows nested rows only while their parent is open", () => {
    renderWithStrings(<Accounts />);
    expect(rows()).toEqual(["Example Air Traffic"]);
    expect(row("Example Air Traffic")).toHaveAttribute("aria-expanded", "false");
  });

  it("opens and closes rows from the keyboard", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Accounts />);
    await user.tab();
    expect(row("Example Air Traffic")).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(rows()).toEqual(["Example Air Traffic", "Tower", "Enroute"]);
    await user.keyboard("{ArrowDown}{ArrowRight}");
    expect(rows()).toEqual(["Example Air Traffic", "Tower", "Ground", "Enroute"]);
    await user.keyboard("{ArrowLeft}");
    expect(rows()).toEqual(["Example Air Traffic", "Tower", "Enroute"]);
    // Left on a closed row moves to its parent.
    await user.keyboard("{ArrowLeft}");
    expect(row("Example Air Traffic")).toHaveFocus();
  });

  it("names the chevron from the catalog by what it will do, and toggles with it", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Accounts />);
    const chevron = screen.getByRole("button", { name: /Expand/ });
    expect(chevron).toHaveAttribute("data-slot", "tree-item-chevron");
    await user.click(chevron);
    expect(rows()).toContain("Tower");
    expect(screen.getAllByRole("button", { name: /Collapse/ })).toHaveLength(1);
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onExpandedChange = vi.fn();
    renderWithStrings(<Accounts expandedKeys={[]} onExpandedChange={onExpandedChange} />);
    await user.click(screen.getByRole("button", { name: /Expand/ }));
    expect(onExpandedChange).toHaveBeenCalledWith(new Set(["root"]));
    expect(rows()).toEqual(["Example Air Traffic"]);
  });

  it("calls onAction with the row's id", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    renderWithStrings(<Accounts defaultExpandedKeys={["root"]} onAction={onAction} />);
    await user.click(screen.getByText("Enroute"));
    expect(onAction).toHaveBeenCalledWith("enroute");
  });

  it("shows its empty state", () => {
    renderWithStrings(
      <Tree aria-label="Folders" emptyState="No folders yet.">
        {[]}
      </Tree>,
    );
    expect(screen.getByText("No folders yet.")).toHaveAttribute("data-slot", "tree-empty");
  });

  it("renders only catalog messages and the caller's content", () => {
    renderWithStrings(
      <Tree aria-label="«caller»" defaultExpandedKeys={["a"]}>
        <TreeItem id="a" title="«caller»">
          <TreeItem id="b" title="«caller»" />
        </TreeItem>
      </Tree>,
      { marked: true },
    );
    expect(unmarkedText(document.body)).toEqual([]);
  });
});
