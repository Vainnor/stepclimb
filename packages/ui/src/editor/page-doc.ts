// SPDX-License-Identifier: AGPL-3.0-or-later
// The page body the server stores (docs/specs/m1/07-pages-and-the-editor.md B7, B8;
// `stepclimb_core::page_doc`), and `toPageDoc`, which turns the editor's Tiptap JSON into it.
//
// Tiptap's extensions carry attributes the model does not have (a link's `target`, `rel` and
// `class`, an ordered list's `type`, a cell's `align`), which the server refuses as unknown
// attributes. `toPageDoc` keeps, for each node and mark, exactly the attributes the model lists
// and writes each one out, as the server's stored form does, so a body the editor saves is the
// server's canonical form of itself. It changes no value: a value the server refuses is refused
// with its JSON Pointer, which the editor shows at the block.
// crates/stepclimb-core/tests/fixtures/page_doc/editor-document.json pins the shape on both sides.

import type { JSONContent } from "@tiptap/core";

/** The languages a code block may name (`page_doc::CODE_LANGUAGES`). */
export const codeLanguages = [
  "bash",
  "c",
  "cpp",
  "csharp",
  "css",
  "go",
  "html",
  "java",
  "javascript",
  "json",
  "kotlin",
  "markdown",
  "php",
  "python",
  "ruby",
  "rust",
  "sql",
  "swift",
  "typescript",
  "xml",
  "yaml",
] as const;
export type CodeLanguage = (typeof codeLanguages)[number];

/** The stored file types an image may name (`page_doc::IMAGE_TYPES`, B10a). */
export const imageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;

/** The heading levels a body takes; the page title is the level 1. */
export const headingLevels = [2, 3, 4] as const;

/** The largest `colspan` or `rowspan` of a table cell (`page_doc::MAX_CELL_SPAN`). */
export const MAX_CELL_SPAN = 20;

/** The longest transcript speaker label and video title, in characters (B11, B12). */
export const MAX_SPEAKER_CHARS = 40;
export const MAX_EMBED_TITLE_CHARS = 200;

/** A node or mark of a page body as JSON: what the API's `PageDoc` schema describes. */
export interface PageNode {
  type: string;
  attrs?: Record<string, unknown>;
  content?: PageNode[];
  text?: string;
  marks?: PageMark[];
}

export interface PageMark {
  type: string;
  attrs?: Record<string, unknown>;
}

/** A page body: `{ "type": "doc", "content": [...] }`. */
export interface PageDoc extends PageNode {
  type: "doc";
  content: PageNode[];
}

/** Each node's attributes and the value written when the editor holds none. */
const NODE_ATTRS: Record<string, Record<string, unknown>> = {
  heading: { level: 2 },
  orderedList: { start: 1 },
  codeBlock: { language: null },
  tableCell: { colspan: 1, rowspan: 1, colwidth: null },
  tableHeader: { colspan: 1, rowspan: 1, colwidth: null },
  image: { course_file_id: "", alt: "", decorative: false },
  callout: { kind: "info" },
  videoEmbed: { provider: "youtube", video_id: "", title: "" },
  transcriptLine: { speaker: "" },
};

/** `attrs` reduced to the names in `known`, each written out. */
function pick(attrs: Record<string, unknown> | undefined, known: Record<string, unknown>) {
  return Object.fromEntries(
    Object.entries(known).map(([name, fallback]) => [name, attrs?.[name] ?? fallback]),
  );
}

function mark(value: { type: string; attrs?: Record<string, unknown> }): PageMark {
  return value.type === "link"
    ? { type: "link", attrs: { href: value.attrs?.href ?? "" } }
    : { type: value.type };
}

function text(node: JSONContent): PageNode {
  const out: PageNode = { type: "text", text: node.text ?? "" };
  if (node.marks && node.marks.length > 0) out.marks = node.marks.map(mark);
  return out;
}

/** A code block's text, its text nodes joined as one, as the server stores it. */
function codeBlock(node: JSONContent): PageNode {
  const joined = (node.content ?? []).map((child) => child.text ?? "").join("");
  const out: PageNode = { type: "codeBlock", attrs: pick(node.attrs, NODE_ATTRS.codeBlock ?? {}) };
  if (joined !== "") out.content = [{ type: "text", text: joined }];
  return out;
}

function pageNode(node: JSONContent): PageNode {
  const type = node.type ?? "";
  if (type === "text") return text(node);
  if (type === "codeBlock") return codeBlock(node);
  const out: PageNode = { type };
  const known = NODE_ATTRS[type];
  if (known) out.attrs = pick(node.attrs, known);
  if (node.content && node.content.length > 0) out.content = node.content.map(pageNode);
  return out;
}

/** The editor's document as the page body to save. */
export function toPageDoc(json: JSONContent): PageDoc {
  return { type: "doc", content: (json.content ?? []).map(pageNode) };
}

/** The body of a new page: one empty paragraph, as the server creates it. */
export function emptyPageDoc(): PageDoc {
  return { type: "doc", content: [{ type: "paragraph" }] };
}

/**
 * The index of the top-level block a JSON Pointer into a body names (`/content/3/attrs/alt` is
 * block 3), or `undefined` for a pointer at the body itself. A save's pointer starts with
 * `/body`, which `prefix` names.
 */
export function blockIndexOf(pointer: string, prefix = ""): number | undefined {
  if (!pointer.startsWith(`${prefix}/content/`)) return undefined;
  const segment = pointer.slice(prefix.length + "/content/".length).split("/")[0];
  return segment !== undefined && /^\d+$/.test(segment) ? Number(segment) : undefined;
}

/** The two bodies are the same document, whatever the order of their members. */
export function sameDoc(a: JSONContent, b: JSONContent): boolean {
  return stable(a) === stable(b);
}

function stable(value: unknown): string {
  return JSON.stringify(value, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v).sort(([x], [y]) => (x < y ? -1 : 1)))
      : v,
  );
}
