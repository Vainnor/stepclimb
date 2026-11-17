// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor emits the body the server stores (spec 07 B7, B8, B21). The fixtures live beside the
// server's tests (crates/stepclimb-core/tests/page_doc_editor.rs), which parse the same files: a
// change to the editor's schema or to `page_doc` that the other side does not follow fails there
// or here.
import { Editor } from "@tiptap/core";
import document from "../../../../crates/stepclimb-core/tests/fixtures/page_doc/editor-document.json";
import { type BlockKind, blockKinds, insertBlock } from "./blocks";
import { pageExtensions } from "./extensions";
import { blockIndexOf, emptyPageDoc, type PageDoc, toPageDoc } from "./page-doc";

const t = (id: string) => id;

function editorWith(content: PageDoc): Editor {
  return new Editor({ extensions: pageExtensions({ t }), content, injectCSS: false });
}

const INSERTED = "../../../../crates/stepclimb-core/tests/fixtures/page_doc/editor-inserted.json";

/** The details an image and an embed are inserted with. */
const details: Partial<Record<BlockKind, Record<string, unknown>>> = {
  image: {
    course_file_id: "01900000-0000-7000-8000-0000000000f1",
    alt: "A diagram",
    decorative: false,
  },
  videoEmbed: { provider: "vimeo", video_id: "76979871", title: "A briefing" },
};

describe("toPageDoc", () => {
  it("gives back every node, mark and attribute of the model unchanged", () => {
    const editor = editorWith(document as PageDoc);
    expect(toPageDoc(editor.getJSON())).toEqual(document);
    editor.destroy();
  });

  it("writes every block the slash menu inserts in the server's stored form", async () => {
    const editor = editorWith(emptyPageDoc());
    for (const kind of blockKinds) {
      // Each block takes the place of a new empty paragraph at the end of the document.
      editor.commands.insertContentAt(editor.state.doc.content.size, { type: "paragraph" });
      editor.commands.focus("end");
      insertBlock(editor, kind, { speaker: "Speaker", attrs: details[kind] });
    }
    const body = toPageDoc(editor.getJSON());
    const kinds = body.content.map((block) => block.type);
    expect(new Set(kinds)).toEqual(
      new Set([
        "paragraph",
        "heading",
        "bulletList",
        "orderedList",
        "blockquote",
        "codeBlock",
        "callout",
        "table",
        "transcript",
        "horizontalRule",
        "image",
        "videoEmbed",
      ]),
    );
    await expect(`${JSON.stringify(body, null, 2)}\n`).toMatchFileSnapshot(INSERTED);
    editor.destroy();
  });

  it("keeps only the model's attributes, written out", () => {
    const tiptap = {
      type: "doc",
      content: [
        {
          type: "orderedList",
          attrs: { start: 2, type: "a" },
          content: [{ type: "listItem", content: [{ type: "paragraph" }] }],
        },
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "a",
              marks: [
                {
                  type: "link",
                  attrs: { href: "https://example.com/", target: "_blank", rel: "noopener", class: null },
                },
              ],
            },
          ],
        },
        {
          type: "table",
          content: [
            {
              type: "tableRow",
              content: [
                {
                  type: "tableCell",
                  attrs: { colspan: 1, rowspan: 1, colwidth: null, align: "left" },
                  content: [{ type: "paragraph" }],
                },
              ],
            },
          ],
        },
        { type: "codeBlock", content: [{ type: "text", text: "a" }, { type: "text", text: "b" }] },
        { type: "image", attrs: { course_file_id: "x", alt: "y" } },
      ],
    };
    expect(toPageDoc(tiptap)).toEqual({
      type: "doc",
      content: [
        {
          type: "orderedList",
          attrs: { start: 2 },
          content: [{ type: "listItem", content: [{ type: "paragraph" }] }],
        },
        {
          type: "paragraph",
          content: [
            {
              type: "text",
              text: "a",
              marks: [{ type: "link", attrs: { href: "https://example.com/" } }],
            },
          ],
        },
        {
          type: "table",
          content: [
            {
              type: "tableRow",
              content: [
                {
                  type: "tableCell",
                  attrs: { colspan: 1, rowspan: 1, colwidth: null },
                  content: [{ type: "paragraph" }],
                },
              ],
            },
          ],
        },
        { type: "codeBlock", attrs: { language: null }, content: [{ type: "text", text: "ab" }] },
        { type: "image", attrs: { course_file_id: "x", alt: "y", decorative: false } },
      ],
    });
  });

  it("passes a node the model does not know to the server, which names it", () => {
    expect(toPageDoc({ type: "doc", content: [{ type: "question" }] })).toEqual({
      type: "doc",
      content: [{ type: "question" }],
    });
  });
});

describe("blockIndexOf", () => {
  it("reads the top-level block from a refusal's pointer", () => {
    expect(blockIndexOf("/body/content/3/attrs/course_file_id", "/body")).toBe(3);
    expect(blockIndexOf("/content/0", "")).toBe(0);
    expect(blockIndexOf("/body", "/body")).toBeUndefined();
    expect(blockIndexOf("/body/type", "/body")).toBeUndefined();
    expect(blockIndexOf("/content/x/attrs", "")).toBeUndefined();
  });
});
