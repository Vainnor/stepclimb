// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings } from "../test/strings";
import { AlertDialog } from "./alert-dialog";
import { Button } from "./button";
import { DialogTrigger } from "./dialog";

function Example({ onConfirm }: { onConfirm: () => void }) {
  return (
    <DialogTrigger>
      <Button intent="danger">Delete</Button>
      <AlertDialog
        title="Delete Ground control?"
        confirmLabel="Delete module"
        intent="danger"
        onConfirm={onConfirm}
      >
        Its four pages and their progress are deleted. This cannot be undone.
      </AlertDialog>
    </DialogTrigger>
  );
}

describe("AlertDialog", () => {
  it("is an alertdialog whose confirm names its action in the danger intent (T12)", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example onConfirm={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const dialog = screen.getByRole("alertdialog", { name: "Delete Ground control?" });
    expect(dialog).toBeVisible();
    expect(dialog).toHaveAccessibleDescription(
      "Its four pages and their progress are deleted. This cannot be undone.",
    );
    const confirm = screen.getByRole("button", { name: "Delete module" });
    expect(confirm).toHaveAttribute("data-slot", "alert-dialog-confirm");
    expect(confirm).toHaveClass("text-danger-fg");
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
  });

  it("starts on Cancel, so Enter on arrival does not run the action", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    renderWithStrings(<Example onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveFocus();
    expect(cancel).toHaveAttribute("data-slot", "alert-dialog-cancel");
    await user.keyboard("{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("runs the action and closes on confirm", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    renderWithStrings(<Example onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete module" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("ignores a press on the scrim and closes on Escape without the action", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    renderWithStrings(<Example onConfirm={onConfirm} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(document.querySelector('[data-slot="dialog-overlay"]') as Element);
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    screen.getByRole("button", { name: "Cancel" }).focus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("uses the primary intent unless asked for danger, and works controlled", async () => {
    const onOpenChange = vi.fn();
    renderWithStrings(
      <AlertDialog
        title="Publish Ground control?"
        confirmLabel="Publish module"
        onConfirm={vi.fn()}
        isOpen
        onOpenChange={onOpenChange}
      >
        Learners see it at once.
      </AlertDialog>,
    );
    expect(screen.getByRole("button", { name: "Publish module" })).toHaveClass("bg-brand");
    await userEvent.setup().click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
