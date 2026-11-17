// SPDX-License-Identifier: AGPL-3.0-or-later
// Mounts a Tiptap editor with the page schema into an element and re-renders the component on
// every transaction, so the toolbars follow the selection. Tiptap's own React binding is not used:
// the editor is mounted once, its node views are plain DOM, and this is all the binding it needs.
import { Editor } from "@tiptap/core";
import { type RefObject, useEffect, useReducer, useRef, useState } from "react";
import { pageExtensions, type Translate } from "./extensions";
import { type PageDoc, toPageDoc } from "./page-doc";

export interface PageEditorOptions {
  /** The document to start with. */
  initial: PageDoc;
  t: Translate;
  /** The content element's attributes: its name, description and classes. */
  attributes: Record<string, string>;
  /** Called with the body after every change the author makes. */
  onChange: (doc: PageDoc) => void;
  /** Called on text input before the editor applies it; return true to take it. */
  onTextInput: (editor: Editor, text: string) => boolean;
  isDisabled: boolean;
}

export function usePageEditor(
  mount: RefObject<HTMLDivElement | null>,
  options: PageEditorOptions,
): Editor | null {
  const [editor, setEditor] = useState<Editor | null>(null);
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  // Read through a ref so the editor, created once, always calls the current props.
  const current = useRef(options);
  current.current = options;

  useEffect(() => {
    const element = mount.current;
    if (!element) return;
    const created: Editor = new Editor({
      element,
      // Forwards the node views' own lookups, which pass literal ids (extensions.ts).
      extensions: pageExtensions({ t: (id, values) => current.current.t(id, values) }), // strings-check-ignore
      content: current.current.initial,
      injectCSS: false,
      editable: !current.current.isDisabled,
      editorProps: {
        attributes: current.current.attributes,
        handleTextInput: (view, _from, _to, text): boolean =>
          !view.composing && current.current.onTextInput(created, text),
      },
    });
    created.on("transaction", rerender);
    created.on("update", () => current.current.onChange(toPageDoc(created.getJSON())));
    setEditor(created);
    return () => {
      created.destroy();
      setEditor(null);
    };
  }, [mount]);

  useEffect(() => {
    editor?.setOptions({
      editorProps: { ...editor.options.editorProps, attributes: options.attributes },
    });
  }, [editor, options.attributes]);

  useEffect(() => {
    editor?.setEditable(!options.isDisabled, false);
  }, [editor, options.isDisabled]);

  return editor;
}
