// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WIDE_QUERY } from "../lib/breakpoints";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { Button } from "./button";
import { DialogTrigger } from "./dialog";
import { Sheet, type SheetPlacement } from "./sheet";

function wide(isWide: boolean) {
  stubMatchMedia((query) => (query === WIDE_QUERY ? isWide : false));
}

function Example({ placement }: { placement?: SheetPlacement }) {
  return (
    <DialogTrigger>
      <Button>Edit page</Button>
      <Sheet
        title="Edit page"
        placement={placement}
        footer={({ close }) => (
          <Button intent="primary" onPress={close}>
            Save page
          </Button>
        )}
      >
        Page settings
      </Sheet>
    </DialogTrigger>
  );
}

async function open() {
  await userEvent.setup().click(screen.getByRole("button", { name: "Edit page" }));
  return screen.getByRole("dialog", { name: "Edit page" });
}

describe("Sheet", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("opens at the inline end at 768 px and wider, with its data-slots", async () => {
    wide(true);
    renderWithStrings(<Example />);
    const dialog = await open();
    expect(dialog).toHaveAttribute("data-slot", "sheet-dialog");
    const panel = dialog.closest('[data-slot="sheet"]');
    expect(panel).toHaveAttribute("data-placement", "end");
    expect(panel).toHaveClass("border-s", "h-full", "pb-[env(safe-area-inset-bottom)]");
    expect(dialog.querySelector('[data-slot="sheet-body"]')).toHaveTextContent("Page settings");
    expect(dialog.querySelector('[data-slot="sheet-footer"]')).not.toBeNull();
  });

  it("opens from the bottom under 768 px, inside the bottom safe area", async () => {
    wide(false);
    renderWithStrings(<Example />);
    const panel = (await open()).closest('[data-slot="sheet"]');
    expect(panel).toHaveAttribute("data-placement", "bottom");
    expect(panel).toHaveClass("rounded-t-md", "pb-[env(safe-area-inset-bottom)]");
  });

  it("keeps a fixed placement at every width", async () => {
    wide(false);
    renderWithStrings(<Example placement="end" />);
    expect((await open()).closest('[data-slot="sheet"]')).toHaveAttribute("data-placement", "end");
  });

  it("closes from its close button, its footer and Escape, returning focus", async () => {
    wide(true);
    const user = userEvent.setup();
    renderWithStrings(<Example />);
    const trigger = screen.getByRole("button", { name: "Edit page" });
    await open();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await open();
    await user.click(screen.getByRole("button", { name: "Save page" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await open();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("works controlled", async () => {
    wide(true);
    const onOpenChange = vi.fn();
    renderWithStrings(
      <Sheet title="Edit page" isOpen onOpenChange={onOpenChange}>
        Page settings
      </Sheet>,
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "Close" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
