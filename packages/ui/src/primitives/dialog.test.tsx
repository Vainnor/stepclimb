// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { ThemeProvider } from "../theme/theme-provider";
import { Button } from "./button";
import { Dialog, DialogBody, DialogFooter, DialogTrigger } from "./dialog";
import { TextField } from "./text-field";

function Example(props: { isDismissable?: boolean; onOpenChange?: (open: boolean) => void }) {
  return (
    <DialogTrigger onOpenChange={props.onOpenChange}>
      <Button>Rename module</Button>
      <Dialog title="Rename module" isDismissable={props.isDismissable}>
        {({ close }) => (
          <>
            <DialogBody>
              <TextField label="Name" defaultValue="Ground control" />
            </DialogBody>
            <DialogFooter>
              <Button onPress={close}>Cancel</Button>
              <Button intent="primary" onPress={close}>
                Save name
              </Button>
            </DialogFooter>
          </>
        )}
      </Dialog>
    </DialogTrigger>
  );
}

describe("Dialog", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is a modal dialog named by its title, with its data-slots", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    const dialog = screen.getByRole("dialog", { name: "Rename module" });
    expect(dialog).toHaveAttribute("data-slot", "dialog");
    expect(dialog.closest('[data-slot="dialog-modal"]')).not.toBeNull();
    expect(dialog.closest('[data-slot="dialog-overlay"]')).toHaveClass("bg-overlay");
    expect(dialog.querySelector('[data-slot="dialog-body"]')).not.toBeNull();
    expect(dialog.querySelector('[data-slot="dialog-footer"]')).not.toBeNull();
    expect(screen.getByRole("button", { name: "Close" })).toHaveAttribute(
      "data-slot",
      "dialog-close",
    );
  });

  it("traps focus inside, closes on Escape and returns focus to its trigger (T12)", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    const trigger = screen.getByRole("button", { name: "Rename module" });
    await user.click(trigger);
    const dialog = screen.getByRole("dialog");
    // Tab past the last control comes back to the first: focus never leaves the dialog.
    for (let i = 0; i < 6; i++) {
      await user.tab();
      expect(dialog.contains(document.activeElement)).toBe(true);
    }
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("closes from its close button and from an action given `close`", async () => {
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    await user.click(screen.getByRole("button", { name: "Save name" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes on a press on the scrim unless isDismissable is false", async () => {
    const user = userEvent.setup();
    const { unmount } = renderWithStrings(<Example />);
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    await user.click(document.querySelector('[data-slot="dialog-overlay"]') as Element);
    expect(screen.queryByRole("dialog")).toBeNull();
    unmount();
    renderWithStrings(<Example isDismissable={false} />);
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    await user.click(document.querySelector('[data-slot="dialog-overlay"]') as Element);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("works controlled and uncontrolled", async () => {
    const onOpenChange = vi.fn();
    const { rerender } = renderWithStrings(
      <Dialog title="Rename module" isOpen={false} onOpenChange={onOpenChange}>
        Body
      </Dialog>,
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    rerender(
      <Dialog title="Rename module" isOpen onOpenChange={onOpenChange}>
        Body
      </Dialog>,
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    // Controlled: it stays open until the caller closes it.
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("opens by default with defaultOpen", () => {
    renderWithStrings(
      <Dialog title="Opened" defaultOpen>
        Body
      </Dialog>,
    );
    expect(screen.getByRole("dialog", { name: "Opened" })).toBeInTheDocument();
  });

  it("draws in the surrounding theme", async () => {
    stubMatchMedia(() => false);
    const user = userEvent.setup();
    renderWithStrings(
      <ThemeProvider theme="dark" brand="vzdc">
        <Example />
      </ThemeProvider>,
    );
    await user.click(screen.getByRole("button", { name: "Rename module" }));
    const overlay = screen.getByRole("dialog").closest('[data-slot="dialog-overlay"]');
    expect(overlay).toHaveAttribute("data-sc-theme", "dark");
    expect(overlay).toHaveAttribute("data-sc-brand", "vzdc");
  });
});
