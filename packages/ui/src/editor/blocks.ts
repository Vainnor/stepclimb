// SPDX-License-Identifier: AGPL-3.0-or-later
// The blocks an author inserts from the slash menu or the toolbar's "Insert" menu, and the
// commands the block toolbar runs on the block holding the selection
// (docs/specs/m1/07-pages-and-the-editor.md B8, B21).
import type { Editor, JSONContent } from "@tiptap/core";
import type { Node as PmNode, ResolvedPos } from "@tiptap/pm/model";
import { NodeSelection, TextSelection } from "@tiptap/pm/state";

/** Every block the slash menu offers, in its order. */
export const blockKinds = [
  "paragraph",
  "heading2",
  "heading3",
  "heading4",
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
] as const;
export type BlockKind = (typeof blockKinds)[number];

/** Blocks that need more than a click to insert: the editor asks for their details first. */
export const blocksWithDetails: readonly BlockKind[] = ["image", "videoEmbed"];

/** The blocks a list item, blockquote, callout or table cell may hold (`page_doc` NESTED). */
const NESTABLE: readonly BlockKind[] = ["paragraph", "bulletList", "orderedList", "codeBlock"];

const paragraph: JSONContent = { type: "paragraph" };
const item: JSONContent = { type: "listItem", content: [paragraph] };

function cell(type: "tableHeader" | "tableCell"): JSONContent {
  return { type, attrs: { colspan: 1, rowspan: 1, colwidth: null }, content: [paragraph] };
}

function table(): JSONContent {
  const row = (type: "tableHeader" | "tableCell") => ({
    type: "tableRow",
    content: [cell(type), cell(type), cell(type)],
  });
  return { type: "table", content: [row("tableHeader"), row("tableCell"), row("tableCell")] };
}

/** The content to insert for `kind`; `attrs` are an image's or an embed's details. */
export function blockContent(
  kind: BlockKind,
  options: { speaker: string; attrs?: Record<string, unknown> },
): JSONContent[] {
  switch (kind) {
    case "paragraph":
      return [paragraph];
    case "heading2":
    case "heading3":
    case "heading4":
      return [{ type: "heading", attrs: { level: Number(kind.slice(-1)) } }];
    case "bulletList":
      return [{ type: "bulletList", content: [item] }];
    case "orderedList":
      return [{ type: "orderedList", attrs: { start: 1 }, content: [item] }];
    case "blockquote":
      return [{ type: "blockquote", content: [paragraph] }];
    case "codeBlock":
      return [{ type: "codeBlock", attrs: { language: null } }];
    case "callout":
      return [{ type: "callout", attrs: { kind: "info" }, content: [paragraph] }];
    case "table":
      return [table()];
    case "transcript":
      return [
        {
          type: "transcript",
          content: [{ type: "transcriptLine", attrs: { speaker: options.speaker } }],
        },
      ];
    case "horizontalRule":
      return [{ type: "horizontalRule" }, paragraph];
    case "image":
    case "videoEmbed":
      return [{ type: kind, attrs: options.attrs ?? {} }, paragraph];
  }
}

function isEmptyParagraph($from: ResolvedPos): boolean {
  return $from.parent.type.name === "paragraph" && $from.parent.content.size === 0;
}

/**
 * Where `kind` goes: in place of the empty paragraph holding the caret when the block may stand
 * there, otherwise after the block holding the caret (a top-level block after its top-level
 * ancestor, since it may not be nested).
 */
export function insertRange(editor: Editor, kind: BlockKind): { from: number; to: number } {
  const { $from } = editor.state.selection;
  if ($from.depth === 0) {
    const at = editor.state.selection.from;
    return { from: at, to: editor.state.selection.to };
  }
  const nestable = NESTABLE.includes(kind);
  const depth = nestable ? $from.depth : 1;
  if (isEmptyParagraph($from) && (nestable || $from.depth === 1)) {
    return { from: $from.before($from.depth), to: $from.after($from.depth) };
  }
  const after = $from.after(depth);
  return { from: after, to: after };
}

/** Inserts `kind` (B8) and leaves the caret in it, or after it when it holds no text. */
export function insertBlock(
  editor: Editor,
  kind: BlockKind,
  options: { speaker: string; attrs?: Record<string, unknown> },
): boolean {
  return editor
    .chain()
    .focus()
    .insertContentAt(insertRange(editor, kind), blockContent(kind, options), {
      updateSelection: true,
    })
    .scrollIntoView()
    .run();
}

/** The block the block toolbar acts on: the top-level block, and the innermost special one. */
export interface BlockContext {
  /** The top-level block: its index and node. */
  index: number;
  top: PmNode;
  /** The innermost node with its own settings, and its position. */
  node: PmNode;
  pos: number;
}

const WITH_SETTINGS = new Set([
  "image",
  "videoEmbed",
  "transcriptLine",
  "codeBlock",
  "callout",
  "tableCell",
  "tableHeader",
  "listItem",
  "heading",
]);

/** The block holding the selection, or `undefined` for none (an empty selection at the end). */
export function blockContext(editor: Editor): BlockContext | undefined {
  const { selection, doc } = editor.state;
  const $from = selection.$from;
  if (selection instanceof NodeSelection && $from.depth === 0) {
    return {
      index: $from.index(0),
      top: selection.node,
      node: selection.node,
      pos: selection.from,
    };
  }
  if ($from.depth === 0) return undefined;
  const top = doc.child($from.index(0));
  for (let depth = $from.depth; depth >= 1; depth -= 1) {
    const node = $from.node(depth);
    if (WITH_SETTINGS.has(node.type.name)) {
      return { index: $from.index(0), top, node, pos: $from.before(depth) };
    }
  }
  return { index: $from.index(0), top, node: top, pos: $from.before(1) };
}

/** Sets attributes of the node at `pos` without moving the selection. */
export function setAttrsAt(editor: Editor, pos: number, attrs: Record<string, unknown>): void {
  const { tr } = editor.state;
  const node = tr.doc.nodeAt(pos);
  if (!node) return;
  const selected = editor.state.selection instanceof NodeSelection;
  tr.setNodeMarkup(pos, undefined, { ...node.attrs, ...attrs });
  // An atom's markup is replaced whole, which would drop the selection of it, and with it the
  // block toolbar the author is typing in.
  if (selected) tr.setSelection(NodeSelection.create(tr.doc, pos));
  editor.view.dispatch(tr);
}

/** The position of the top-level block `index`. */
function topPos(doc: PmNode, index: number): number {
  let pos = 0;
  for (let i = 0; i < index; i += 1) pos += doc.child(i).nodeSize;
  return pos;
}

/** Moves the top-level block `index` one place up (`-1`) or down (`1`); false at an end. */
export function moveBlock(editor: Editor, index: number, by: -1 | 1): boolean {
  const { doc, tr } = editor.state;
  const target = index + by;
  if (target < 0 || target >= doc.childCount) return false;
  const node = doc.child(index);
  const from = topPos(doc, index);
  tr.delete(from, from + node.nodeSize);
  const to = topPos(tr.doc, target);
  tr.insert(to, node);
  const inside = Math.min(to + 1, tr.doc.content.size);
  tr.setSelection(
    node.isAtom ? NodeSelection.create(tr.doc, to) : TextSelection.near(tr.doc.resolve(inside)),
  );
  editor.view.dispatch(tr.scrollIntoView());
  return true;
}

/** Deletes the top-level block `index`; the document keeps at least an empty paragraph. */
export function deleteBlock(editor: Editor, index: number): void {
  const { doc, tr, schema } = editor.state;
  const from = topPos(doc, index);
  tr.delete(from, from + doc.child(index).nodeSize);
  if (tr.doc.childCount === 0) tr.insert(0, schema.nodes.paragraph?.create() ?? []);
  const at = Math.min(from, tr.doc.content.size);
  tr.setSelection(TextSelection.near(tr.doc.resolve(at)));
  editor.view.dispatch(tr.scrollIntoView());
  editor.view.focus();
}

/** Selects the top-level block `index` and scrolls it into view: the target of a refusal. */
export function revealBlock(editor: Editor, index: number): void {
  const { doc, tr } = editor.state;
  if (index >= doc.childCount) return;
  const pos = topPos(doc, index);
  const node = doc.child(index);
  tr.setSelection(
    node.isAtom ? NodeSelection.create(doc, pos) : TextSelection.near(doc.resolve(pos + 1)),
  );
  editor.view.dispatch(tr.scrollIntoView());
  editor.view.focus();
}
