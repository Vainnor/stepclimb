// SPDX-License-Identifier: AGPL-3.0-or-later
// Pasted content is cut to the page document model (spec 07 B7, B8, T13: the editor's schema
// refuses pasting a `<script>`). The result is written to a fixture the server's tests parse
// (crates/stepclimb-core/tests/page_doc_editor.rs), so what survives a paste is a body the
// server stores as it is.
import { Editor } from "@tiptap/core";
import document from "../../../../crates/stepclimb-core/tests/fixtures/page_doc/editor-document.json";
import { pageExtensions } from "./extensions";
import { emptyPageDoc, type PageDoc, type PageNode, toPageDoc } from "./page-doc";

// jsdom has no ClipboardEvent, which ProseMirror's pasteHTML creates to stand for the paste.
if (!("ClipboardEvent" in globalThis)) {
  Object.assign(globalThis, { ClipboardEvent: class extends Event {} });
}

/** Every text node's text and every string attribute in `node`. */
function texts(node: PageNode): string[] {
  const own = [
    ...(node.text === undefined ? [] : [node.text]),
    ...Object.values(node.attrs ?? {}).filter((v): v is string => typeof v === "string"),
  ];
  return [...own, ...(node.content ?? []).flatMap(texts)];
}

/** Every table cell in `node`. */
function cells(node: PageNode): PageNode[] {
  const own = node.type === "tableCell" || node.type === "tableHeader" ? [node] : [];
  return [...own, ...(node.content ?? []).flatMap(cells)];
}

const PASTED = "../../../../crates/stepclimb-core/tests/fixtures/page_doc/editor-pasted.json";

function withEditor(run: (editor: Editor) => void): PageDoc {
  const editor = new Editor({
    extensions: pageExtensions({ t: (id) => id }),
    content: emptyPageDoc(),
    injectCSS: false,
  });
  run(editor);
  const doc = toPageDoc(editor.getJSON());
  editor.destroy();
  return doc;
}

const HOSTILE = [
  "<h1>Title</h1>",
  `<p onclick="steal()" style="color:red" class="x">a<script>alert(1)</script>b</p>`,
  `<img src="https://evil.example/x.png" onerror="alert(2)">`,
  `<p><a href="javascript:alert(3)">bad</a> <a href="HTTPS://Example.com" target="_blank" rel="opener" class="c">good</a></p>`,
  "<p>nul\u0000he\u0007re\u009f</p>",
  `<pre><code class="language-perl">x</code></pre>`,
  `<pre><code class="language-rust">y</code></pre>`,
  `<ol type="a" start="2"><li>x</li></ol>`,
  `<table><tr><td style="text-align:right" colwidth="40" data-colwidth="40">c</td></tr></table>`,
  "<h5>five</h5>",
  "<blockquote><h2>quoted heading</h2></blockquote>",
  "<ul><li><p>item</p><h3>heading in a list</h3><table><tr><td>t</td></tr></table></li></ul>",
  `<iframe src="https://evil.example"></iframe><style>body{}</style><svg onload="alert(4)"></svg>`,
  `<figure data-course-file-id="not-a-file"></figure>`,
  `<div data-provider="youtube" data-video-id="short">embed</div>`,
  `<figure data-course-file-id="01900000-0000-7000-8000-0000000000f1" data-alt="a\u0007lt" data-decorative="false" data-src="https://evil.example/x.png" onclick="steal()"></figure>`,
  `<div data-callout="" data-kind="sparkle"><p>invented kind</p></div>`,
  `<table><tr><td colspan="two" rowspan="1.5">a</td><td colspan="99">b</td></tr></table>`,
  `<div data-transcript=""><p data-speaker="${"S".repeat(60)}\u0001">line</p></div>`,
].join("");

describe("paste", () => {
  it("keeps only the model's nodes, marks and attributes", async () => {
    const doc = withEditor((editor) => editor.view.pasteHTML(HOSTILE));
    const text = JSON.stringify(doc);
    for (const gone of [
      "alert",
      "steal",
      "evil",
      "onclick",
      "onerror",
      "perl",
      "_blank",
      "align",
      "style",
    ]) {
      expect(text).not.toContain(gone);
    }
    // JSON.stringify writes U+007F and U+0080 to U+009F as they are, so look at the text itself.
    const control = (t: string) =>
      [...t].some((c) => c !== "\t" && c !== "\n" && /\p{Cc}/u.test(c));
    expect(texts(doc).filter(control)).toEqual([]);
    const types = new Set(text.match(/"type":"[a-zA-Z]+"/g));
    // Only the figure copied from a page, with its file id, pastes as an image, and only with
    // the image's own attributes.
    expect(doc.content.filter((block) => block.type === "image")).toEqual([
      {
        type: "image",
        attrs: {
          course_file_id: "01900000-0000-7000-8000-0000000000f1",
          alt: "alt",
          decorative: false,
        },
      },
    ]);
    const callouts = doc.content.filter((block) => block.type === "callout");
    expect(callouts.map((block) => block.attrs)).toEqual([{ kind: "info" }]);
    const spans = cells(doc).map((cell) => [cell.attrs?.colspan, cell.attrs?.rowspan]);
    expect(spans.every(([c, r]) => Number.isInteger(c) && Number.isInteger(r))).toBe(true);
    expect(spans).toContainEqual([1, 1]);
    expect(spans.flat().every((n) => (n as number) >= 1 && (n as number) <= 20)).toBe(true);
    const speakers = JSON.stringify(doc).match(/"speaker":"([^"]*)"/g) ?? [];
    expect(speakers).toEqual([`"speaker":"${"S".repeat(40)}"`]);
    expect(types).not.toContain('"type":"videoEmbed"');
    expect(text).not.toContain("data-src");
    // An `<h1>` and an `<h5>` are no heading level a body takes: they paste as paragraphs.
    expect(doc.content[0]).toEqual({
      type: "paragraph",
      content: [{ type: "text", text: "Title" }],
    });
    expect(text).toContain('"marks":[{"type":"link","attrs":{"href":"https://example.com/"}}]');
    expect(text).toContain('"text":"nulhere"');
    // A list item and a quote hold only paragraphs, lists and code: a heading or a table pasted
    // into one lands after it.
    const nestedTypes = (node: { type: string; content?: unknown[] }): string[] =>
      (node.content ?? []).flatMap((child) => {
        const c = child as { type: string; content?: unknown[] };
        return [c.type, ...nestedTypes(c)];
      });
    for (const block of doc.content.filter(
      (b) => b.type === "bulletList" || b.type === "blockquote",
    )) {
      expect(nestedTypes(block)).not.toContain("heading");
      expect(nestedTypes(block)).not.toContain("table");
    }
    await expect(`${JSON.stringify(doc, null, 2)}\n`).toMatchFileSnapshot(PASTED);
  });

  it("pastes a copy of every block back as it was", () => {
    const source = new Editor({
      extensions: pageExtensions({ t: (id) => id }),
      content: document as PageDoc,
      injectCSS: false,
    });
    const html = source.getHTML();
    source.destroy();
    const pasted = withEditor((editor) => editor.view.pasteHTML(html));
    expect(pasted).toEqual(document);
  });

  it("takes control characters out of pasted plain text", () => {
    const doc = withEditor((editor) => editor.view.pasteText("one\u0000\u001btwo\tthree"));
    expect(doc.content).toEqual([
      { type: "paragraph", content: [{ type: "text", text: "onetwo\tthree" }] },
    ]);
  });
});
