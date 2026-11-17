// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor's schema: Tiptap with only the nodes and marks of the page document model
// (docs/specs/m1/07-pages-and-the-editor.md B8, B21; `stepclimb_core::page_doc`), placed where the
// model places them. A blockquote, a callout, a list item and a table cell hold only the *nested*
// blocks (paragraphs, lists, code blocks); headings, rules, tables, images, embeds, callouts and
// transcripts are top-level blocks. Pasted content is parsed into this schema, so markup it does
// not name (a script, an `onclick`, an `<img>` from another site, an `<h1>`) does not survive a
// paste, and `sanitizePaste` also takes out the control characters the server refuses.
import { Extension, type Extensions, Mark, Node, type NodeViewRendererProps } from "@tiptap/core";
import Blockquote from "@tiptap/extension-blockquote";
import Bold from "@tiptap/extension-bold";
import Code from "@tiptap/extension-code";
import CodeBlock from "@tiptap/extension-code-block";
import Document from "@tiptap/extension-document";
import HardBreak from "@tiptap/extension-hard-break";
import Heading from "@tiptap/extension-heading";
import HorizontalRule from "@tiptap/extension-horizontal-rule";
import Italic from "@tiptap/extension-italic";
import { BulletList, ListItem, OrderedList } from "@tiptap/extension-list";
import Paragraph from "@tiptap/extension-paragraph";
import Strike from "@tiptap/extension-strike";
import Subscript from "@tiptap/extension-subscript";
import Superscript from "@tiptap/extension-superscript";
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table";
import Text from "@tiptap/extension-text";
import Underline from "@tiptap/extension-underline";
import { Dropcursor, Gapcursor, UndoRedo } from "@tiptap/extensions";
import { Fragment, type Node as PmNode, Slice } from "@tiptap/pm/model";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { type CalloutKind, calloutKinds } from "../learning/callout";
import type { MessageValues } from "../strings/strings";
import { linkTarget } from "./link-target";
import {
  codeLanguages,
  headingLevels,
  MAX_CELL_SPAN,
  MAX_EMBED_TITLE_CHARS,
  MAX_SPEAKER_CHARS,
} from "./page-doc";
import { type VideoProvider, videoIdIsValid } from "./video-url";

/** The blocks a nested position takes. */
const NESTED = "nested";

/** Looks up a message: the editor's `t` from `useStepclimbStrings()`. */
export type Translate = (id: string, values?: MessageValues) => string;

/** A control character (Unicode category Cc). */
const CONTROL = /\p{Cc}/u;

/** A pasted label without control characters, cut to `max` characters. */
function label(value: string | null, max: number): string {
  return [...(value ?? "").replace(ATTR_CONTROL, "")].slice(0, max).join("");
}

/** A course file id: what an image copied from a page carries. */
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Control characters the server refuses in an attribute: every one. */
const ATTR_CONTROL = /\p{Cc}/gu;

/** `text` without the control characters a page's text may not hold. */
export function plainText(text: string): string {
  return [...text].filter((c) => c === "\t" || c === "\n" || !CONTROL.test(c)).join("");
}

const NestedParagraph = Paragraph.extend({ group: `block ${NESTED}` });
const NestedBulletList = BulletList.extend({ group: `block list ${NESTED}` });

/** An ordered list with `start` only: Tiptap's `type` is not in the model. */
const PageOrderedList = OrderedList.extend({
  group: `block list ${NESTED}`,
  addAttributes() {
    return {
      start: {
        default: 1,
        parseHTML: (el: HTMLElement) => {
          const start = Number.parseInt(el.getAttribute("start") ?? "", 10);
          return Number.isInteger(start) && start >= 0 ? start : 1;
        },
      },
    };
  },
});

const PageListItem = ListItem.extend({ content: `paragraph ${NESTED}*` });
const PageBlockquote = Blockquote.extend({ content: `${NESTED}+` });

/** A code block whose language is one of the listed ones, or none. */
const PageCodeBlock = CodeBlock.extend({
  group: `block ${NESTED}`,
  addAttributes() {
    return {
      language: {
        default: null,
        parseHTML: (el: HTMLElement) => {
          const code = el.firstElementChild ?? el;
          const named = [...code.classList]
            .filter((name) => name.startsWith("language-"))
            .map((name) => name.slice("language-".length))[0];
          return (codeLanguages as readonly string[]).includes(named ?? "") ? named : null;
        },
        rendered: false,
      },
    };
  },
});

/** Cell attributes: spans, and `colwidth` always `null`, since M1 keeps no column widths. */
/** A pasted cell span as the integer 1 to 20 the model takes, or 1. */
function span(el: HTMLElement, name: "colspan" | "rowspan"): number {
  const value = Number(el.getAttribute(name) ?? "1");
  return Number.isInteger(value) && value >= 1 && value <= MAX_CELL_SPAN ? value : 1;
}

const cellAttributes = () => ({
  colspan: { default: 1, parseHTML: (el: HTMLElement) => span(el, "colspan") },
  rowspan: { default: 1, parseHTML: (el: HTMLElement) => span(el, "rowspan") },
  colwidth: { default: null, parseHTML: () => null, rendered: false },
});
const PageTableCell = TableCell.extend({ content: `${NESTED}+`, addAttributes: cellAttributes });
const PageTableHeader = TableHeader.extend({
  content: `${NESTED}+`,
  addAttributes: cellAttributes,
});

/** A link with `href` only; the reader adds `rel` and `target` (B9). */
export const PageLink = Mark.create({
  name: "link",
  priority: 1000,
  inclusive: false,
  keepOnSplit: false,
  addAttributes() {
    return {
      href: {
        default: null,
        parseHTML: (el: HTMLElement) => linkTarget(el.getAttribute("href") ?? ""),
      },
    };
  },
  parseHTML() {
    return [
      {
        tag: "a[href]",
        getAttrs: (el: HTMLElement) => (linkTarget(el.getAttribute("href") ?? "") ? null : false),
      },
    ];
  },
  renderHTML({ HTMLAttributes }) {
    return ["a", { ...HTMLAttributes, rel: "noopener noreferrer nofollow" }, 0];
  },
});

/** A DOM node view with a label the author cannot edit above its content. */
function labelledView(
  tag: "div" | "figure",
  slot: string,
  label: (node: PmNode) => string,
  editable: boolean,
) {
  return ({ node }: NodeViewRendererProps) => {
    const dom = document.createElement(tag);
    dom.dataset.slot = slot;
    const caption = document.createElement("span");
    caption.dataset.slot = `${slot}-label`;
    caption.contentEditable = "false";
    caption.textContent = label(node);
    dom.append(caption);
    let contentDOM: HTMLElement | undefined;
    if (editable) {
      contentDOM = document.createElement("div");
      contentDOM.dataset.slot = `${slot}-content`;
      dom.append(contentDOM);
    }
    const sync = (current: PmNode) => {
      for (const [name, value] of Object.entries(current.attrs)) {
        dom.setAttribute(`data-${name.replaceAll("_", "-")}`, String(value));
      }
      caption.textContent = label(current);
    };
    sync(node);
    return {
      dom,
      contentDOM,
      update: (updated: PmNode) => {
        if (updated.type !== node.type) return false;
        sync(updated);
        return true;
      },
    };
  };
}

interface Labels {
  t: Translate;
}

const callout = (t: Translate) =>
  Node.create({
    name: "callout",
    group: "block",
    content: `${NESTED}+`,
    defining: true,
    addAttributes() {
      return {
        kind: {
          default: "info" satisfies CalloutKind,
          parseHTML: (el: HTMLElement) => {
            const kind = el.getAttribute("data-kind");
            return (calloutKinds as readonly (string | null)[]).includes(kind) ? kind : "info";
          },
        },
      };
    },
    parseHTML: () => [{ tag: "div[data-callout]" }],
    renderHTML: ({ node }) => ["div", { "data-callout": "", "data-kind": node.attrs.kind }, 0],
    addNodeView: () =>
      labelledView(
        "div",
        "editor-callout",
        (node) => t("callout.kind", { kind: node.attrs.kind }),
        true,
      ),
  });

const image = (t: Translate) =>
  Node.create({
    name: "image",
    group: "block",
    atom: true,
    draggable: true,
    addAttributes() {
      return {
        course_file_id: {
          default: "",
          parseHTML: (el: HTMLElement) => el.getAttribute("data-course-file-id") ?? "",
        },
        alt: {
          default: "",
          parseHTML: (el: HTMLElement) =>
            (el.getAttribute("data-alt") ?? "").replace(ATTR_CONTROL, ""),
        },
        decorative: {
          default: false,
          parseHTML: (el: HTMLElement) => el.getAttribute("data-decorative") === "true",
        },
      };
    },
    parseHTML: () => [
      {
        tag: "figure[data-course-file-id]",
        getAttrs: (el: HTMLElement) =>
          UUID.test(el.getAttribute("data-course-file-id") ?? "") ? null : false,
      },
    ],
    renderHTML: ({ node }) => [
      "figure",
      {
        "data-course-file-id": node.attrs.course_file_id,
        "data-alt": node.attrs.alt,
        "data-decorative": String(node.attrs.decorative),
      },
    ],
    addNodeView: () =>
      labelledView(
        "figure",
        "editor-image",
        (node) =>
          node.attrs.decorative
            ? t("editor.image.decorative")
            : t("editor.image.label", { alt: node.attrs.alt }),
        false,
      ),
  });

const videoEmbed = (t: Translate) =>
  Node.create({
    name: "videoEmbed",
    group: "block",
    atom: true,
    draggable: true,
    addAttributes() {
      return {
        provider: {
          default: "youtube",
          parseHTML: (el: HTMLElement) => el.getAttribute("data-provider"),
        },
        video_id: { default: "", parseHTML: (el: HTMLElement) => el.getAttribute("data-video-id") },
        title: {
          default: "",
          parseHTML: (el: HTMLElement) =>
            label(el.getAttribute("data-title"), MAX_EMBED_TITLE_CHARS),
        },
      };
    },
    parseHTML: () => [
      {
        tag: "div[data-provider][data-video-id]",
        getAttrs: (el: HTMLElement) => {
          const provider = el.getAttribute("data-provider");
          const id = el.getAttribute("data-video-id") ?? "";
          const known = provider === "youtube" || provider === "vimeo";
          return known && videoIdIsValid(provider as VideoProvider, id) ? null : false;
        },
      },
    ],
    renderHTML: ({ node }) => [
      "div",
      {
        "data-provider": node.attrs.provider,
        "data-video-id": node.attrs.video_id,
        "data-title": node.attrs.title,
      },
    ],
    addNodeView: () =>
      labelledView(
        "div",
        "editor-video",
        (node) =>
          t("editor.video.label", { provider: node.attrs.provider, title: node.attrs.title }),
        false,
      ),
  });

const transcript = Node.create({
  name: "transcript",
  group: "block",
  content: "transcriptLine+",
  defining: true,
  parseHTML: () => [{ tag: "div[data-transcript]" }],
  renderHTML: () => ["div", { "data-transcript": "", "data-slot": "editor-transcript" }, 0],
});

const transcriptLine = Node.create({
  name: "transcriptLine",
  content: "inline*",
  defining: true,
  addAttributes() {
    return {
      speaker: {
        default: "",
        keepOnSplit: true,
        parseHTML: (el: HTMLElement) => label(el.getAttribute("data-speaker"), MAX_SPEAKER_CHARS),
      },
    };
  },
  // Ahead of the paragraph's `p` rule, which would otherwise take the line and leave its speaker.
  parseHTML: () => [{ tag: "p[data-speaker]", priority: 60 }],
  renderHTML: ({ node }) => ["p", { "data-speaker": node.attrs.speaker }, 0],
  addNodeView: () =>
    labelledView("div", "editor-transcript-line", (node) => String(node.attrs.speaker), true),
});

/** `fragment` with the control characters the server refuses taken out of its text. */
function cleanFragment(fragment: Fragment): Fragment {
  const nodes: PmNode[] = [];
  fragment.forEach((node) => {
    if (node.isText) {
      const text = plainText(node.text ?? "");
      if (text !== "") nodes.push(node.type.schema.text(text, node.marks));
    } else {
      nodes.push(node.copy(cleanFragment(node.content)));
    }
  });
  return Fragment.from(nodes);
}

/** Takes the control characters the server refuses out of pasted and dropped content. */
export const sanitizePaste = Extension.create({
  name: "sanitizePaste",
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey("sanitizePaste"),
        props: {
          transformPastedText: (text) => plainText(text.replaceAll("\r\n", "\n")),
          transformPasted: (slice) =>
            new Slice(cleanFragment(slice.content), slice.openStart, slice.openEnd),
        },
      }),
    ];
  },
});

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type=hidden])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[contenteditable=true]",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

/**
 * The focusable elements after (`1`, nearest first) or before (`-1`, nearest first) `from` in
 * document order, outside it.
 */
function focusablesBeside(from: HTMLElement, direction: 1 | -1): HTMLElement[] {
  const dom = globalThis.Node;
  const side = direction === 1 ? dom.DOCUMENT_POSITION_FOLLOWING : dom.DOCUMENT_POSITION_PRECEDING;
  const beside = [...document.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) =>
      !el.closest("[hidden], [inert]") &&
      !from.contains(el) &&
      from.compareDocumentPosition(el) & side,
  );
  return direction === 1 ? beside : beside.reverse();
}

/** Focuses the first of `candidates` that takes focus (one that is drawn and enabled). */
function focusFirst(candidates: HTMLElement[]): boolean {
  for (const el of candidates) {
    el.focus();
    if (document.activeElement === el) return true;
  }
  return false;
}

/**
 * Escape, then Tab, leaves the page text: inside a table Tab moves between cells and inside a
 * list it indents, so without this a keyboard user could not Tab out of either. Escape arms the
 * next Tab (or Shift+Tab) to move focus to the next (or previous) control outside the text; any
 * other key disarms it.
 */
export const escapeToLeave = Extension.create({
  name: "escapeToLeave",
  priority: 1000,
  addProseMirrorPlugins() {
    let armed = false;
    return [
      new Plugin({
        key: new PluginKey("escapeToLeave"),
        props: {
          handleKeyDown: (view, event) => {
            if (event.key === "Escape") {
              armed = true;
              return false;
            }
            if (event.key !== "Tab" || !armed) {
              armed = false;
              return false;
            }
            armed = false;
            // Nothing to move to: leave the text all the same, and the next Tab starts over.
            if (!focusFirst(focusablesBeside(view.dom, event.shiftKey ? -1 : 1))) view.dom.blur();
            return true;
          },
        },
      }),
    ];
  },
});

/** Every extension of the page editor. */
export function pageExtensions({ t }: Labels): Extensions {
  return [
    Document,
    NestedParagraph,
    Text,
    Heading.configure({ levels: [...headingLevels] }),
    NestedBulletList,
    PageOrderedList,
    PageListItem,
    PageBlockquote,
    HorizontalRule,
    HardBreak,
    PageCodeBlock,
    Table.configure({ resizable: false }),
    TableRow,
    PageTableHeader,
    PageTableCell,
    image(t),
    callout(t),
    videoEmbed(t),
    transcript,
    transcriptLine,
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    PageLink,
    Subscript,
    Superscript,
    UndoRedo,
    Gapcursor,
    Dropcursor,
    sanitizePaste,
    escapeToLeave,
  ];
}
