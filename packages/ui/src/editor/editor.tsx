// SPDX-License-Identifier: AGPL-3.0-or-later
// Editor: the page editor (docs/specs/m1/07-pages-and-the-editor.md B8 to B12, B21; plan, UI tab
// -> Library architecture, `src/editor`). A Tiptap editor holding only the page document model's
// nodes and marks, with a toolbar, a slash menu (`/` at the start of an empty paragraph opens the
// block list) and a block toolbar for the block holding the selection. It gives the body in the
// form the server stores (`toPageDoc`), and shows a refusal the server answered with at the
// top-level block its JSON Pointer names. It saves nothing itself: the screen around it does.
import type { Editor as TiptapEditor } from "@tiptap/core";
import {
  type CSSProperties,
  type ReactNode,
  type RefObject,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
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
import { emptyPageDoc, type PageDoc, sameDoc, toPageDoc } from "./page-doc";
import { problemKey, problemPlugin } from "./problem-mark";
import { usePageEditor } from "./use-page-editor";

export const editor = tv({
  slots: {
    root: "flex min-w-0 flex-col rounded-md border border-line bg-raised",
    // The actions and the toolbar stay at the top of the scrolling view at every width. The block
    // bar below them stays too from 768 px; on a phone it scrolls away, so the sticky part is no
    // taller than two rows when the software keyboard takes half the screen.
    chrome: "sticky top-0 z-20 rounded-t-md border-b border-line bg-raised",
    block: "z-10 border-b border-line bg-raised md:sticky md:top-(--editor-chrome-h)",
    actions:
      "flex flex-wrap items-center justify-end gap-x-3 gap-y-2 border-b border-line-soft px-3 py-2",
    problem: "flex flex-wrap items-center gap-3 border-b border-line-soft px-3 py-2",
    content: "min-h-64 px-4 py-6 sm:px-8",
    surface: "mx-auto min-h-48 rounded-sm px-2 py-1",
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
  /** Puts the caret at the start of the page once the editor is drawn. */
  autoFocus?: boolean;
  /** The server's refusal of the last save, shown above the content and at its block. */
  problem?: EditorProblem;
  /**
   * Uploads an image as a course file of the page's course (B10). Without it, "Image" is not
   * offered.
   */
  onUploadImage?: (file: File, onProgress: (fraction: number) => void) => Promise<UploadedImage>;
  /**
   * The screen's own controls, such as Save and the save state, drawn at the top of the editor's
   * sticky bar, so they stay in view while the author scrolls or types on a phone.
   */
  actions?: ReactNode;
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
  autoFocus = false,
  problem,
  onUploadImage,
  actions,
  className,
}: EditorProps) {
  const { t } = useStepclimbStrings();
  const mount = useRef<HTMLDivElement>(null);
  const chrome = useRef<HTMLDivElement>(null);
  const problemId = useId();
  const keysId = useId();
  const [menuOpen, setMenuOpen] = useState(false);
  const [panel, setPanel] = useState<Panel>();
  // Set while a menu opened by `/` is open; if the author leaves it with Escape, the `/` is typed.
  const slash = useRef<"open" | "escape" | "chosen" | undefined>(undefined);
  // The last body this editor gave `onChange`: a `value` that is that body is the editor's own.
  const emitted = useRef<PageDoc | undefined>(undefined);
  const initial = useRef(value ?? defaultValue ?? emptyPageDoc());

  const attributes = useMemo(() => {
    const attrs: Record<string, string> = {
      role: "textbox",
      "aria-multiline": "true",
      "aria-label": ariaLabel ?? t("editor.contentLabel"),
      "aria-describedby": keysId,
      "data-slot": "prose",
      class: prose({ className: styles.surface() }),
    };
    if (problem) attrs["aria-describedby"] = `${problemId} ${keysId}`;
    return attrs;
  }, [ariaLabel, t, problem, problemId, keysId]);

  const instance = usePageEditor(mount, {
    initial: initial.current,
    t,
    attributes,
    isDisabled,
    onChange: (doc) => {
      emitted.current = doc;
      onChange?.(doc);
    },
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
    if (!instance || value === undefined || value === emitted.current) return;
    if (!sameDoc(value, toPageDoc(instance.getJSON()))) {
      instance.commands.setContent(value, { emitUpdate: false });
    }
  }, [instance, value]);

  useEffect(() => {
    if (instance && autoFocus) instance.commands.focus("start");
  }, [instance, autoFocus]);

  useSlashEscape(menuOpen, slash);

  useMarkProblemBlock(instance, problem?.block);
  const chromeHeight = useHeight(chrome, instance !== null);

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
    // Escape after `/`: the author meant the character. A click elsewhere types nothing.
    if (slash.current === "escape") instance.chain().focus().insertContent("/").run();
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
    <div
      data-slot="editor"
      className={styles.root({ className })}
      style={{ "--editor-chrome-h": `${chromeHeight}px` } as CSSProperties}
    >
      {instance ? (
        <div data-slot="editor-chrome" ref={chrome} className={styles.chrome()}>
          {actions === undefined ? null : (
            <div data-slot="editor-actions" className={styles.actions()}>
              {actions}
            </div>
          )}
          <EditorToolbar
            editor={instance}
            isInsertOpen={menuOpen}
            onInsertOpenChange={menuChange}
            onInsert={insert}
            disabledBlocks={onUploadImage ? [] : ["image"]}
          />
        </div>
      ) : null}
      {instance && (context || panel) ? (
        <div data-slot="editor-block" className={styles.block()}>
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
      <p id={keysId} hidden>
        {t("editor.keys")}
      </p>
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

/** The element's height in pixels, followed as it wraps or its content changes. */
function useHeight(ref: RefObject<HTMLElement | null>, mounted: boolean): number {
  const [height, setHeight] = useState(0);
  useEffect(() => {
    const element = ref.current;
    if (!mounted || !element) return;
    const observer = new ResizeObserver(() => setHeight(element.offsetHeight));
    observer.observe(element);
    setHeight(element.offsetHeight);
    return () => observer.disconnect();
  }, [ref, mounted]);
  return height;
}

/** Records an Escape pressed while the slash menu is open, so its close types the `/`. */
function useSlashEscape(open: boolean, slash: RefObject<"open" | "escape" | "chosen" | undefined>) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && slash.current === "open") slash.current = "escape";
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, slash]);
}
