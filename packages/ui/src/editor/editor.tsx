// SPDX-License-Identifier: AGPL-3.0-or-later
// Editor: the page editor (docs/specs/m1/07-pages-and-the-editor.md B8 to B12, B21; plan, UI tab
// -> Library architecture, `src/editor`). A Tiptap editor holding only the page document model's
// nodes and marks, with a toolbar, a slash menu (`/` at the start of an empty paragraph opens the
// block list) and a block toolbar for the block holding the selection. It gives the body in the
// form the server stores (`toPageDoc`), and shows a refusal the server answered with at the
// top-level block its JSON Pointer names. It saves nothing itself: the screen around it does.
import type { Editor as TiptapEditor } from "@tiptap/core";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { tv } from "../lib/tv";
import { FormError } from "../patterns/form-error";
import { Button } from "../primitives/button";
import { prose } from "../primitives/prose";
import { useStepclimbStrings } from "../strings/strings";
import { BlockToolbar } from "./block-toolbar";
import {
  type BlockKind,
  blockContext,
  blocksWithDetails,
  insertBlock,
  revealBlock,
} from "./blocks";
import { EditorToolbar } from "./editor-toolbar";
import { ImagePanel, type UploadedImage, VideoPanel } from "./insert-panel";
import { emptyPageDoc, type PageDoc } from "./page-doc";
import { problemKey, problemPlugin } from "./problem-mark";
import { sameDoc, usePageEditor } from "./use-page-editor";

export const editor = tv({
  slots: {
    root: "flex min-w-0 flex-col rounded-md border border-line bg-raised",
    chrome: "sticky top-0 z-10 rounded-t-md border-b border-line bg-raised",
    problem: "flex flex-wrap items-center gap-3 border-b border-line-soft px-3 py-2",
    content: "min-h-64 px-4 py-6 sm:px-8",
    surface: "mx-auto min-h-48 outline-none",
  },
});

const styles = editor();

/** A refusal of the body: the message, and the top-level block it is about, if one. */
export interface EditorProblem {
  message: string;
  /** The server's code, shown when the app has no message of its own for it. */
  code?: string;
  /** The index of the top-level block, from the refusal's pointer (`blockIndexOf`). */
  block?: number;
}

export interface EditorProps {
  /** The body, controlled. A value that differs from the editor's replaces its content. */
  value?: PageDoc;
  /** The body to start with, uncontrolled; an empty paragraph when neither is given. */
  defaultValue?: PageDoc;
  /** Called with the body, in the form the server stores, after every change. */
  onChange?: (doc: PageDoc) => void;
  /** The editing area's accessible name; the strings catalog's "Page text" by default. */
  "aria-label"?: string;
  /** Read only, such as while a save is in flight. */
  isDisabled?: boolean;
  /** The server's refusal of the last save, shown above the content and at its block. */
  problem?: EditorProblem;
  /**
   * Uploads an image as a course file of the page's course (B10). Without it, "Image" is not
   * offered.
   */
  onUploadImage?: (file: File, onProgress: (fraction: number) => void) => Promise<UploadedImage>;
  /** Layout only. */
  className?: string;
}

type Panel = "image" | "videoEmbed" | undefined;

/** Opens the slash menu for `/` typed at the start of an empty paragraph. */
function isSlash(editor: TiptapEditor, text: string): boolean {
  const { selection } = editor.state;
  const parent = selection.$from.parent;
  return (
    text === "/" && selection.empty && parent.type.name === "paragraph" && parent.content.size === 0
  );
}

export function Editor({
  value,
  defaultValue,
  onChange,
  "aria-label": ariaLabel,
  isDisabled = false,
  problem,
  onUploadImage,
  className,
}: EditorProps) {
  const { t } = useStepclimbStrings();
  const mount = useRef<HTMLDivElement>(null);
  const problemId = useId();
  const [menuOpen, setMenuOpen] = useState(false);
  const [panel, setPanel] = useState<Panel>();
  // Set while a menu opened by `/` is open; if it closes with no block chosen, the `/` is typed.
  const slash = useRef<"open" | "chosen" | undefined>(undefined);
  const initial = useRef(value ?? defaultValue ?? emptyPageDoc());

  const attributes = useMemo(() => {
    const attrs: Record<string, string> = {
      role: "textbox",
      "aria-multiline": "true",
      "aria-label": ariaLabel ?? t("editor.contentLabel"),
      "data-slot": "prose",
      class: prose({ className: styles.surface() }),
    };
    if (problem) attrs["aria-describedby"] = problemId;
    return attrs;
  }, [ariaLabel, t, problem, problemId]);

  const instance = usePageEditor(mount, {
    initial: initial.current,
    t,
    attributes,
    isDisabled,
    onChange: (doc) => onChange?.(doc),
    onTextInput: (current, text) => {
      if (!isSlash(current, text)) return false;
      slash.current = "open";
      setMenuOpen(true);
      return true;
    },
  });

  // A controlled value that is not what the editor holds replaces its content: a restored draft
  // or a reloaded version. The editor's own changes come back equal and are left alone.
  useEffect(() => {
    if (!instance || value === undefined) return;
    if (!sameDoc(value, instance.getJSON())) {
      instance.commands.setContent(value, { emitUpdate: false });
    }
  }, [instance, value]);

  useMarkProblemBlock(instance, problem?.block);

  const speaker = t("editor.transcript.speaker");
  const insert = (kind: BlockKind) => {
    if (!instance) return;
    if (slash.current === "open") slash.current = "chosen";
    if (blocksWithDetails.includes(kind)) setPanel(kind as Panel);
    else insertBlock(instance, kind, { speaker });
  };
  const menuChange = (open: boolean) => {
    setMenuOpen(open);
    if (open || !instance) return;
    // Escape after `/`: the author meant the character.
    if (slash.current === "open") instance.chain().focus().insertContent("/").run();
    slash.current = undefined;
  };
  const insertWithDetails = (attrs: Record<string, unknown>) => {
    if (!instance || !panel) return;
    insertBlock(instance, panel, { speaker, attrs });
    setPanel(undefined);
  };
  const cancelPanel = () => {
    setPanel(undefined);
    instance?.commands.focus();
  };

  const context = instance ? blockContext(instance) : undefined;
  return (
    <div data-slot="editor" className={styles.root({ className })}>
      {instance ? (
        <div data-slot="editor-chrome" className={styles.chrome()}>
          <EditorToolbar
            editor={instance}
            isInsertOpen={menuOpen}
            onInsertOpenChange={menuChange}
            onInsert={insert}
            disabledBlocks={onUploadImage ? [] : ["image"]}
          />
          {context ? (
            <BlockToolbar
              editor={instance}
              context={context}
              count={instance.state.doc.childCount}
            />
          ) : null}
          {panel === "image" && onUploadImage ? (
            <ImagePanel
              onUpload={onUploadImage}
              onInsert={insertWithDetails}
              onCancel={cancelPanel}
            />
          ) : null}
          {panel === "videoEmbed" ? (
            <VideoPanel onInsert={insertWithDetails} onCancel={cancelPanel} />
          ) : null}
        </div>
      ) : null}
      {problem ? (
        <div data-slot="editor-problem" id={problemId} className={styles.problem()}>
          <FormError code={problem.code}>{problem.message}</FormError>
          {instance && problem.block !== undefined ? (
            <Button size="sm" onPress={() => revealBlock(instance, problem.block ?? 0)}>
              {t("editor.problem.show")}
            </Button>
          ) : null}
        </div>
      ) : null}
      <div data-slot="editor-content" ref={mount} className={styles.content()} />
    </div>
  );
}

/**
 * Marks the top-level block a refusal is about with `data-invalid` through a decoration, which
 * the editor keeps on the block while the author edits it, until the refusal goes.
 */
function useMarkProblemBlock(instance: TiptapEditor | null, block: number | undefined) {
  useEffect(() => {
    if (!instance || block === undefined) return;
    const plugin = problemPlugin(block);
    instance.registerPlugin(plugin);
    return () => {
      if (!instance.isDestroyed) instance.unregisterPlugin(problemKey);
    };
  }, [instance, block]);
}
