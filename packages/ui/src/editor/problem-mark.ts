// SPDX-License-Identifier: AGPL-3.0-or-later
// A decoration on the top-level block a server refusal names (docs/specs/m1/07-pages-and-the-editor.md
// B7, B10a: the editor shows the refusal at the block), so the block is outlined and stays
// outlined while the author fixes it.
import type { Node as PmNode } from "@tiptap/pm/model";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

export const problemKey = new PluginKey("pageProblem");

/** The decoration on block `index` of `doc`, or none when the document is shorter. */
function decorate(doc: PmNode, index: number): DecorationSet {
  if (index >= doc.childCount) return DecorationSet.empty;
  let pos = 0;
  for (let i = 0; i < index; i += 1) pos += doc.child(i).nodeSize;
  const node = doc.child(index);
  return DecorationSet.create(doc, [
    Decoration.node(pos, pos + node.nodeSize, { "data-invalid": "" }),
  ]);
}

export function problemPlugin(index: number): Plugin {
  return new Plugin({
    key: problemKey,
    state: {
      init: (_config, state) => decorate(state.doc, index),
      apply: (tr, set) => set.map(tr.mapping, tr.doc),
    },
    props: {
      decorations(state) {
        return problemKey.getState(state) as DecorationSet | undefined;
      },
    },
  });
}
