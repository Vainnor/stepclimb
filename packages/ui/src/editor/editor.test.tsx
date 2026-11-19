// SPDX-License-Identifier: AGPL-3.0-or-later
// The page editor's behavior (docs/specs/m1/07-pages-and-the-editor.md B10 to B12, B21, T13): the
// slash menu inserts each block with the keyboard only, the toolbars change the selection's
// marks and block, links are checked as the server checks them, images and embeds ask for their
// details first, and a refusal from the server is shown at its block.

import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Editor as TiptapEditor } from "@tiptap/core";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { type BlockKind, blockKinds, blocksWithDetails } from "./blocks";
import { Editor, type EditorProps } from "./editor";
import { emptyPageDoc, type PageDoc } from "./page-doc";

function renderEditor(props: Partial<EditorProps> = {}, options = { marked: false }) {
  const changes: PageDoc[] = [];
  const view = renderWithStrings(
    <Editor defaultValue={emptyPageDoc()} onChange={(doc) => changes.push(doc)} {...props} />,
    options,
  );
  return { ...view, changes, last: () => changes.at(-1) };
}

function content(): HTMLElement {
  return screen.getByRole("textbox", { name: "Page text" });
}

/** Types `/` into the empty paragraph, as the author's keyboard would, and waits for the menu. */
async function openSlashMenu() {
  const user = userEvent.setup();
  await user.click(content());
  await user.keyboard("/");
  const menu = await screen.findByRole("menu", { name: "Insert" });
  await waitFor(() => expect(within(menu).getAllByRole("menuitem")[0]).toHaveFocus());
  return { user, menu };
}

const TYPE: Record<BlockKind, string> = {
  paragraph: "paragraph",
  heading2: "heading",
  heading3: "heading",
  heading4: "heading",
  bulletList: "bulletList",
  orderedList: "orderedList",
  blockquote: "blockquote",
  codeBlock: "codeBlock",
  callout: "callout",
  table: "table",
  transcript: "transcript",
  horizontalRule: "horizontalRule",
  image: "image",
  videoEmbed: "videoEmbed",
};

// jsdom lays nothing out; ProseMirror measures the caret to scroll it into view.
beforeAll(() => {
  document.elementFromPoint = () => null;
  const empty = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getClientRects = empty;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});

// Wide: menus open as popovers (`MENU_POPOVER_QUERY`), not bottom sheets.
beforeEach(() => stubMatchMedia(() => true));
afterEach(() => vi.unstubAllGlobals());

describe("the slash menu", () => {
  const direct = blockKinds.filter((kind) => !blocksWithDetails.includes(kind));

  it.each(direct)("inserts %s with the keyboard only", async (kind) => {
    const { last } = renderEditor();
    const { user } = await openSlashMenu();
    const steps = blockKinds.indexOf(kind);
    await user.keyboard(`${"{ArrowDown}".repeat(steps)}{Enter}`);
    await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
    // A paragraph in place of the empty paragraph changes nothing.
    const body = last() ?? emptyPageDoc();
    const types = body?.content.map((block) => block.type) ?? [];
    expect(types[0]).toBe(TYPE[kind]);
    if (kind.startsWith("heading")) {
      expect(body?.content[0]?.attrs).toEqual({ level: Number(kind.slice(-1)) });
    }
    // The caret is back in the page, inside or after the new block.
    await waitFor(() => expect(content()).toHaveFocus());
  });

  it("opens where the author typed before the editor read the moved caret", async () => {
    const heading = {
      type: "heading",
      attrs: { level: 2 },
      content: [{ type: "text", text: "Before you taxi" }],
    };
    const { last } = renderEditor({
      defaultValue: { type: "doc", content: [heading, { type: "paragraph" }] },
    });
    const editor = (content() as HTMLElement & { editor: TiptapEditor }).editor;
    // The selection is still in the heading; the browser put the `/` into the empty paragraph
    // after it, and ProseMirror asks about that range, as on a DOM change read before the
    // `selectionchange` event.
    const at = editor.state.doc.child(0).nodeSize + 1;
    expect(editor.state.selection.$from.parent.type.name).toBe("heading");
    act(() => {
      const view = editor.view;
      const deflt = () => view.state.tr.insertText("/", at, at);
      const taken = view.someProp("handleTextInput", (f) => f(view, at, at, "/", deflt));
      if (!taken) view.dispatch(deflt());
    });
    const menu = await screen.findByRole("menu", { name: "Insert" });
    await waitFor(() => expect(within(menu).getAllByRole("menuitem")[0]).toHaveFocus());
    const user = userEvent.setup();
    await user.keyboard(`${"{ArrowDown}".repeat(blockKinds.indexOf("callout"))}{Enter}`);
    await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
    expect(last()?.content.map((block) => block.type)).toEqual(["heading", "callout"]);
    expect(last()?.content[0]).toEqual(heading);
  });

  it("types / when the author leaves the menu with Escape", async () => {
    const { last } = renderEditor();
    const { user } = await openSlashMenu();
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(last()?.content).toEqual([
        { type: "paragraph", content: [{ type: "text", text: "/" }] },
      ]),
    );
    await waitFor(() => expect(content()).toHaveFocus());
  });

  it("types nothing when the menu closes any other way than Escape", async () => {
    const { last } = renderEditor();
    const { user } = await openSlashMenu();
    await user.click(document.body);
    await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
    expect(last()).toBeUndefined();
  });

  it("asks for an embed's address and title, then inserts it by id", async () => {
    const { last } = renderEditor();
    const { user } = await openSlashMenu();
    // "Image" is disabled without an upload route, so the arrow keys pass over it.
    await user.keyboard(`${"{ArrowDown}".repeat(blockKinds.indexOf("videoEmbed") - 1)}{Enter}`);
    const panel = await screen.findByRole("form", { name: "Insert a video" });
    const address = within(panel).getByRole("textbox", { name: /Video address/ });
    await user.type(address, "https://vimeo.com/abc");
    await user.click(within(panel).getByRole("button", { name: "Insert video" }));
    expect(within(panel).getByText("Enter a YouTube or Vimeo video address.")).toBeInTheDocument();
    expect(within(panel).getByText("Give the video a title.")).toBeInTheDocument();
    await user.clear(address);
    await user.type(address, "https://youtu.be/dQw4w9WgXcQ");
    await user.type(within(panel).getByRole("textbox", { name: /Title/ }), "Taxi briefing");
    await user.click(within(panel).getByRole("button", { name: "Insert video" }));
    expect(last()?.content[0]).toEqual({
      type: "videoEmbed",
      attrs: { provider: "youtube", video_id: "dQw4w9WgXcQ", title: "Taxi briefing" },
    });
    expect(screen.queryByRole("form", { name: "Insert a video" })).not.toBeInTheDocument();
  });

  it("offers Image only when the editor can upload one", async () => {
    renderEditor();
    const { menu } = await openSlashMenu();
    expect(within(menu).getByRole("menuitem", { name: "Image" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });
});

describe("images", () => {
  const png = new File([new Uint8Array([137, 80, 78, 71])], "diagram.png", { type: "image/png" });

  async function openImagePanel(onUploadImage: EditorProps["onUploadImage"]) {
    const view = renderEditor({ onUploadImage });
    const { user } = await openSlashMenu();
    await user.keyboard(`${"{ArrowDown}".repeat(blockKinds.indexOf("image"))}{Enter}`);
    const panel = await screen.findByRole("form", { name: "Insert an image" });
    // The panel takes focus from the menu that opened it.
    await waitFor(() =>
      expect(within(panel).getByRole("button", { name: "Choose file" })).toHaveFocus(),
    );
    return { ...view, user, panel };
  }

  function fileInput(panel: HTMLElement): HTMLInputElement {
    const input = panel.querySelector<HTMLInputElement>("input[type=file]");
    if (!input) throw new Error("no file input");
    return input;
  }

  it("refuses a file the server did not read as an image (B10a)", async () => {
    const upload = vi.fn().mockResolvedValue({ course_file_id: "f", mime: "application/pdf" });
    const { user, panel } = await openImagePanel(upload);
    await user.upload(fileInput(panel), png);
    expect(
      await within(panel).findByText(
        "This file is not a PNG, JPEG, GIF or WebP image. Choose another file.",
      ),
    ).toBeInTheDocument();
    await user.type(within(panel).getByRole("textbox", { name: /Text alternative/ }), "A diagram");
    await user.click(within(panel).getByRole("button", { name: "Insert image" }));
    expect(screen.queryByRole("form", { name: "Insert an image" })).toBeInTheDocument();
  });

  it("inserts an uploaded image with its text alternative", async () => {
    const id = "01900000-0000-7000-8000-0000000000f1";
    const upload = vi.fn().mockResolvedValue({ course_file_id: id, mime: "image/png" });
    const { user, panel, last } = await openImagePanel(upload);
    await user.upload(fileInput(panel), png);
    await waitFor(() => expect(upload).toHaveBeenCalledWith(png, expect.any(Function)));
    await waitFor(() =>
      expect(within(panel).getByRole("button", { name: "Insert image" })).toBeEnabled(),
    );
    await user.click(within(panel).getByRole("button", { name: "Insert image" }));
    expect(
      within(panel).getByText("Describe the image, or mark it as decorative."),
    ).toBeInTheDocument();
    await user.type(within(panel).getByRole("textbox", { name: /Text alternative/ }), "A diagram");
    await user.click(within(panel).getByRole("button", { name: "Insert image" }));
    expect(last()?.content[0]).toEqual({
      type: "image",
      attrs: { course_file_id: id, alt: "A diagram", decorative: false },
    });
  });
});

describe("the toolbar", () => {
  async function selectAll() {
    const user = userEvent.setup();
    await user.click(content());
    await user.keyboard("{Control>}a{/Control}");
    return user;
  }

  const body: PageDoc = {
    type: "doc",
    content: [{ type: "paragraph", content: [{ type: "text", text: "Readback" }] }],
  };

  it("toggles a mark on the selection", async () => {
    const { last } = renderEditor({ defaultValue: body });
    const user = await selectAll();
    await user.click(screen.getByRole("button", { name: "Bold" }));
    expect(last()?.content[0]?.content?.[0]?.marks).toEqual([{ type: "bold" }]);
    expect(screen.getByRole("button", { name: "Bold" })).toHaveAttribute("aria-pressed", "true");
  });

  it("refuses a link the server would refuse and stores an allowed one in its stored form", async () => {
    const { last } = renderEditor({ defaultValue: body });
    const user = await selectAll();
    await user.click(screen.getByRole("button", { name: "Link" }));
    const dialog = await screen.findByRole("dialog", { name: "Link" });
    const address = within(dialog).getByRole("textbox", { name: /Address/ });
    await user.type(address, "javascript:alert(1)");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(
      within(dialog).getByText(
        "Enter an address starting with https://, http://, mailto: or a single /.",
      ),
    ).toBeInTheDocument();
    await user.clear(address);
    await user.type(address, "HTTPS://Example.com");
    await user.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(last()?.content[0]?.content?.[0]?.marks).toEqual([
      { type: "link", attrs: { href: "https://example.com/" } },
    ]);
  });
});

describe("the block toolbar", () => {
  it("sets a callout's kind", async () => {
    const callout: PageDoc = {
      type: "doc",
      content: [
        {
          type: "callout",
          attrs: { kind: "info" },
          content: [{ type: "paragraph", content: [{ type: "text", text: "x" }] }],
        },
      ],
    };
    const { last } = renderEditor({ defaultValue: callout });
    const user = userEvent.setup();
    await user.click(content());
    const settings = await screen.findByRole("group", { name: "Callout settings" });
    await user.click(within(settings).getByRole("button", { name: /Kind/ }));
    await user.click(await screen.findByRole("option", { name: "Warning" }));
    expect(last()?.content[0]?.attrs).toEqual({ kind: "warning" });
  });

  it("moves and deletes the block", async () => {
    const two: PageDoc = {
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "two" }] },
        { type: "paragraph", content: [{ type: "text", text: "one" }] },
      ],
    };
    const { last } = renderEditor({ defaultValue: two });
    const user = userEvent.setup();
    await user.click(content());
    const settings = await screen.findByRole("group", { name: "Heading 2 settings" });
    expect(within(settings).getByRole("button", { name: "Move block up" })).toBeDisabled();
    await user.click(within(settings).getByRole("button", { name: "Move block down" }));
    expect(last()?.content.map((block) => block.type)).toEqual(["paragraph", "heading"]);
    const moved = screen.getByRole("group", { name: "Heading 2 settings" });
    expect(within(moved).getByRole("button", { name: "Move block down" })).toBeDisabled();
    await user.click(within(moved).getByRole("button", { name: "Delete block" }));
    expect(last()?.content.map((block) => block.type)).toEqual(["paragraph"]);
  });
});

describe("a refusal from the server", () => {
  const two: PageDoc = {
    type: "doc",
    content: [
      { type: "paragraph", content: [{ type: "text", text: "one" }] },
      {
        type: "image",
        attrs: {
          course_file_id: "01900000-0000-7000-8000-0000000000f1",
          alt: "x",
          decorative: false,
        },
      },
    ],
  };

  it("is shown above the page, describes it, and marks its block", async () => {
    renderEditor({
      defaultValue: two,
      problem: { message: "This file is not an image.", code: "page.image_type", block: 1 },
    });
    expect(screen.getByRole("alert")).toHaveTextContent("This file is not an image.");
    await waitFor(() => expect(content().children.item(1)).toHaveAttribute("data-invalid"));
    expect(content().children.item(0)).not.toHaveAttribute("data-invalid");
    expect(content()).toHaveAccessibleDescription(/This file is not an image./);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Show the block" }));
    expect(screen.getByRole("group", { name: "Image settings" })).toBeInTheDocument();
  });
});

describe("controlled", () => {
  it("replaces its content when the value changes from outside", async () => {
    const first: PageDoc = {
      type: "doc",
      content: [{ type: "paragraph", content: [{ type: "text", text: "first" }] }],
    };
    const second: PageDoc = {
      type: "doc",
      content: [{ type: "paragraph", content: [{ type: "text", text: "second" }] }],
    };
    const { rerender } = renderWithStrings(<Editor value={first} />);
    await waitFor(() => expect(content()).toHaveTextContent("first"));
    rerender(<Editor value={second} />);
    await waitFor(() => expect(content()).toHaveTextContent("second"));
  });
});

describe("strings", () => {
  it("draws no text that is not from the catalog", async () => {
    renderEditor({}, { marked: true });
    await waitFor(() =>
      expect(document.querySelector('[data-slot="editor-toolbar"]')).not.toBeNull(),
    );
    const root = document.querySelector<HTMLElement>('[data-slot="editor"]');
    expect(root && unmarkedText(root)).toEqual([]);
  });
});

// Escape, then Tab, out of a table is checked in a browser (web/e2e/page-edit.spec.ts): jsdom
// does not place ProseMirror's selection in a cell on a click.
describe("the keyboard", () => {
  it("names the keys in the page text's description", () => {
    renderEditor();
    expect(content()).toHaveAccessibleDescription(/press Escape, then Tab/);
  });

  it("puts the caret in the page when asked to", async () => {
    renderEditor({ autoFocus: true });
    await waitFor(() => expect(content()).toHaveFocus());
  });
});
