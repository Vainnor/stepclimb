// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { acceptsType, checkFiles, dragCarriesFiles, FileDrop, sizeValues } from "./file-drop";

const pdf = (name = "handout.pdf", size = 1000) =>
  new File([new Uint8Array(size)], name, { type: "application/pdf" });
const png = (name = "diagram.png") => new File([new Uint8Array(10)], name, { type: "image/png" });

const fileInput = (container: HTMLElement) => {
  const input = container.querySelector('input[type="file"]');
  if (!(input instanceof HTMLInputElement)) throw new Error("no file input");
  return input;
};

/** A drop of `files` on `target`, as a browser sends it: dragenter, dragover, then drop. */
async function dropFiles(target: Element, files: File[]) {
  const dataTransfer = {
    dropEffect: "none",
    effectAllowed: "all",
    types: ["Files"],
    files,
    items: files.map((file) => ({ kind: "file", type: file.type, getAsFile: () => file })),
    getData: () => "",
  };
  await act(async () => {
    fireEvent.dragEnter(target, { dataTransfer, clientX: 1, clientY: 1 });
    fireEvent.dragOver(target, { dataTransfer, clientX: 1, clientY: 1 });
    fireEvent.drop(target, { dataTransfer, clientX: 1, clientY: 1 });
  });
}

/** A caller that goes busy once it takes files, as an upload batch does. */
function BusyAfterChoice({ onSelect }: { onSelect: (files: File[]) => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <FileDrop
      isBusy={busy}
      onSelect={(files) => {
        onSelect(files);
        setBusy(true);
      }}
    />
  );
}

describe("acceptsType", () => {
  it("matches MIME types, MIME groups and extensions, ignoring case", () => {
    expect(acceptsType(pdf(), ["application/pdf"])).toBe(true);
    expect(acceptsType(png(), ["image/*"])).toBe(true);
    expect(acceptsType(pdf("NOTES.PDF"), [".pdf"])).toBe(true);
    expect(acceptsType(pdf(), ["image/*", ".docx"])).toBe(false);
    expect(acceptsType(pdf(), undefined)).toBe(true);
    expect(acceptsType(pdf(), [])).toBe(true);
  });

  it("refuses a file with no type against a MIME group", () => {
    const unknown = new File(["x"], "data", { type: "" });
    expect(acceptsType(unknown, ["image/*"])).toBe(false);
  });
});

describe("dragCarriesFiles", () => {
  const items = (...kinds: string[]) =>
    kinds.map((kind) => ({ kind })) as unknown as DataTransferItemList;
  it("is true when any item is a file, or for Safari's bare Files type", () => {
    expect(dragCarriesFiles({ items: items("string", "file"), types: [] })).toBe(true);
    expect(dragCarriesFiles({ items: items(), types: ["Files"] })).toBe(true);
    expect(dragCarriesFiles({ items: items("string"), types: ["text/plain"] })).toBe(false);
    expect(dragCarriesFiles(null)).toBe(false);
  });
});

describe("checkFiles", () => {
  it("splits files by type, then by size", () => {
    const big = pdf("big.pdf", 2001);
    const ok = pdf("ok.pdf", 2000);
    const image = png();
    const result = checkFiles([big, ok, image], {
      acceptedFileTypes: ["application/pdf"],
      maxFileSize: 2000,
    });
    expect(result.accepted).toEqual([ok]);
    expect(result.rejected).toEqual([
      { file: big, reason: "size" },
      { file: image, reason: "type" },
    ]);
  });
});

describe("sizeValues", () => {
  it("picks a decimal unit from kilobytes up", () => {
    expect(sizeValues(25_000_000)).toEqual({ unit: "MB", value: 25 });
    expect(sizeValues(2_500_000_000)).toEqual({ unit: "GB", value: 2.5 });
    expect(sizeValues(500)).toEqual({ unit: "kB", value: 0.5 });
  });
});

describe("FileDrop", () => {
  it("names its zone and button from the catalog, with its data-slots and the size limit", () => {
    const { container } = renderWithStrings(
      <FileDrop onSelect={() => {}} maxFileSize={25_000_000} description="PDF or an image." />,
    );
    expect(container.querySelector('[data-slot="file-drop"]')).not.toBeNull();
    expect(screen.getByRole("button", { name: "Choose file" })).toHaveAttribute(
      "data-slot",
      "file-drop-choose",
    );
    // React Aria's hidden drop button, for pasting a file from the keyboard.
    expect(screen.getByRole("button", { name: /Drop area/ })).toBeInTheDocument();
    expect(screen.getByText("PDF or an image. Up to 25 MB per file.")).toHaveAttribute(
      "data-slot",
      "file-drop-limits",
    );
  });

  it("hands chosen files that pass the checks to onSelect", async () => {
    const user = userEvent.setup({ applyAccept: false });
    const onSelect = vi.fn();
    const { container } = renderWithStrings(
      <FileDrop onSelect={onSelect} acceptedFileTypes={["application/pdf"]} />,
    );
    const file = pdf();
    await user.upload(fileInput(container), file);
    expect(onSelect).toHaveBeenCalledWith([file]);
    expect(screen.getByRole("alert")).toBeEmptyDOMElement();
  });

  it("refuses a file of the wrong type or too large, saying which and why", async () => {
    const user = userEvent.setup({ applyAccept: false });
    const onSelect = vi.fn();
    const onReject = vi.fn();
    const { container } = renderWithStrings(
      <FileDrop
        onSelect={onSelect}
        onReject={onReject}
        allowsMultiple
        acceptedFileTypes={["application/pdf"]}
        maxFileSize={1_000}
      />,
    );
    const ok = pdf("ok.pdf", 1_000);
    const big = pdf("big.pdf", 1_001);
    const image = png();
    await user.upload(fileInput(container), [ok, big, image]);
    expect(onSelect).toHaveBeenCalledWith([ok]);
    expect(onReject).toHaveBeenCalledWith([
      { file: big, reason: "size" },
      { file: image, reason: "type" },
    ]);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("big.pdf is larger than 1 kB. Choose a smaller file.");
    expect(alert).toHaveTextContent(
      "diagram.png is not a type of file this field accepts. Choose another file.",
    );
  });

  it("does not call onSelect when every file is refused", async () => {
    const user = userEvent.setup({ applyAccept: false });
    const onSelect = vi.fn();
    const { container } = renderWithStrings(
      <FileDrop onSelect={onSelect} acceptedFileTypes={["image/*"]} />,
    );
    await user.upload(fileInput(container), pdf());
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("takes a dropped file through the same checks", async () => {
    const onSelect = vi.fn();
    const { container } = renderWithStrings(
      <FileDrop onSelect={onSelect} acceptedFileTypes={["application/pdf"]} />,
    );
    const zone = container.querySelector('[data-slot="file-drop"]');
    if (!zone) throw new Error("no zone");
    const file = pdf();
    await dropFiles(zone, [file]);
    await waitFor(() => expect(onSelect).toHaveBeenCalledWith([file]));
    await dropFiles(zone, [png()]);
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("diagram.png is not a type of file"),
    );
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("refuses a drag that carries no file, such as a link", async () => {
    const onSelect = vi.fn();
    const { container } = renderWithStrings(<FileDrop onSelect={onSelect} />);
    const zone = container.querySelector('[data-slot="file-drop"]');
    if (!zone) throw new Error("no zone");
    const dataTransfer = {
      dropEffect: "none",
      effectAllowed: "all",
      types: ["text/uri-list", "text/plain"],
      files: [],
      items: [
        { kind: "string", type: "text/uri-list", getAsString: (f: (s: string) => void) => f("") },
        { kind: "string", type: "text/plain", getAsString: (f: (s: string) => void) => f("") },
      ],
      getData: () => "https://example.test/",
    };
    await act(async () => {
      fireEvent.dragEnter(zone, { dataTransfer, clientX: 1, clientY: 1 });
      fireEvent.dragOver(zone, { dataTransfer, clientX: 1, clientY: 1 });
    });
    expect(zone).not.toHaveAttribute("data-drop-target");
    await act(async () => {
      fireEvent.drop(zone, { dataTransfer, clientX: 1, clientY: 1 });
    });
    expect(onSelect).not.toHaveBeenCalled();
    // A file drag over the same zone is a drop target.
    const files = { ...dataTransfer, types: ["Files"], items: [{ kind: "file", type: "" }] };
    await act(async () => {
      fireEvent.dragEnter(zone, { dataTransfer: files, clientX: 1, clientY: 1 });
      fireEvent.dragOver(zone, { dataTransfer: files, clientX: 1, clientY: 1 });
    });
    expect(zone).toHaveAttribute("data-drop-target", "true");
  });

  it("refuses several dropped files on a zone that takes one", async () => {
    const onSelect = vi.fn();
    const { container } = renderWithStrings(<FileDrop onSelect={onSelect} />);
    const zone = container.querySelector('[data-slot="file-drop"]');
    if (!zone) throw new Error("no zone");
    await dropFiles(zone, [pdf("a.pdf"), pdf("b.pdf")]);
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Drop or choose one file at a time."),
    );
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("says Choose files when it takes several", () => {
    renderWithStrings(<FileDrop onSelect={() => {}} allowsMultiple />);
    expect(screen.getByRole("button", { name: "Choose files" })).toBeInTheDocument();
    expect(screen.getByText("Drag files here")).toBeInTheDocument();
  });

  it("shows the upload's progress", () => {
    renderWithStrings(
      <FileDrop onSelect={() => {}} upload={{ name: "handout.pdf", fraction: 0.4 }} />,
    );
    const bar = screen.getByRole("progressbar", { name: "Uploading handout.pdf" });
    expect(bar).toHaveAttribute("aria-valuenow", "40");
  });

  it("shows an error from outside, such as the server's refusal", () => {
    renderWithStrings(<FileDrop onSelect={() => {}} errorMessage="The upload failed." />);
    expect(screen.getByRole("alert")).toHaveTextContent("The upload failed.");
  });

  it("keeps focus on its button while busy, and takes no files then", async () => {
    const user = userEvent.setup({ applyAccept: false });
    const onSelect = vi.fn();
    const { container } = renderWithStrings(<BusyAfterChoice onSelect={onSelect} />);
    const choose = screen.getByRole("button", { name: "Choose file" });
    // Focus is on the button when the file picker closes, as after a person chose a file.
    act(() => choose.focus());
    await user.upload(fileInput(container), pdf());
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(choose).toHaveFocus();
    expect(choose).not.toBeDisabled();
    expect(choose).toHaveAttribute("aria-disabled", "true");
    // A file chosen or dropped while busy is not handed on.
    await user.upload(fileInput(container), pdf("second.pdf"));
    await dropFiles(container.querySelector('[data-slot="file-drop"]') as Element, [png()]);
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(choose).toHaveFocus();
  });

  it("disables its button while disabled", () => {
    renderWithStrings(<FileDrop onSelect={() => {}} isDisabled />);
    expect(screen.getByRole("button", { name: "Choose file" })).toBeDisabled();
  });

  it("renders only catalog messages and the caller's content", () => {
    renderWithStrings(
      <FileDrop
        onSelect={() => {}}
        maxFileSize={1}
        description="«caller»"
        upload={{ name: "«caller»", fraction: 0.5 }}
        errorMessage="«caller»"
      />,
      { marked: true },
    );
    expect(unmarkedText(document.body)).toEqual([]);
  });
});
