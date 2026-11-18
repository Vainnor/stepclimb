// SPDX-License-Identifier: AGPL-3.0-or-later
// `@stepclimb/ui/editor`: the page editor of docs/specs/m1/07-pages-and-the-editor.md and the page
// document model it writes. A subpath of its own, so Tiptap and ProseMirror load only in the
// screens that import it.
export { Editor, type EditorProblem, type EditorProps, editor } from "./editor";
export type { UploadedImage } from "./insert-panel";
export { linkTarget } from "./link-target";
export {
  blockIndexOf,
  codeLanguages,
  emptyPageDoc,
  imageTypes,
  type PageDoc,
  type PageMark,
  type PageNode,
  sameDoc,
  toPageDoc,
} from "./page-doc";
