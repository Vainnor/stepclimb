// SPDX-License-Identifier: AGPL-3.0-or-later
// The editor's toolbar (docs/specs/m1/07-pages-and-the-editor.md B21): the text style of the
// block, the marks, the link, the lists, "Insert" (the slash menu's list) and undo and redo. One
// tab stop; arrow keys move along it.
import {
  ArrowClockwiseIcon,
  ArrowCounterClockwiseIcon,
  CaretDownIcon,
  CodeIcon,
  LinkIcon,
  ListBulletsIcon,
  ListNumbersIcon,
  type Icon as PhosphorIcon,
  PlusIcon,
  TextBIcon,
  TextItalicIcon,
  TextStrikethroughIcon,
  TextSubscriptIcon,
  TextSuperscriptIcon,
  TextUnderlineIcon,
} from "@phosphor-icons/react";
import type { Editor } from "@tiptap/core";
import { type FormEvent, useState } from "react";
import { tv } from "../lib/tv";
import { Button } from "../primitives/button";
import { IconButton } from "../primitives/icon-button";
import { Menu, MenuItem, MenuTrigger } from "../primitives/menu";
import { Popover, PopoverDialog, PopoverTrigger } from "../primitives/popover";
import { Separator } from "../primitives/separator";
import { TextField } from "../primitives/text-field";
import { ToggleButton } from "../primitives/toggle-button";
import { Toolbar } from "../primitives/toolbar";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { type BlockKind, blockKinds } from "./blocks";
import { linkTarget } from "./link-target";

export const editorToolbar = tv({
  slots: {
    // One row that scrolls sideways under 768 px, so the sticky bar stays two rows high on a phone.
    root: "flex-nowrap gap-1 overflow-x-auto px-2 py-1.5 md:flex-wrap",
    style: "w-36 justify-between",
    styleValue: "min-w-0 truncate",
    linkForm: "flex w-72 max-w-full flex-col gap-3",
    linkActions: "flex flex-wrap justify-end gap-2",
  },
});

const styles = editorToolbar();

const MARKS: [name: string, icon: PhosphorIcon][] = [
  ["bold", TextBIcon],
  ["italic", TextItalicIcon],
  ["underline", TextUnderlineIcon],
  ["strike", TextStrikethroughIcon],
  ["code", CodeIcon],
  ["subscript", TextSubscriptIcon],
  ["superscript", TextSuperscriptIcon],
];

/** The text styles of a text block, as Select keys. */
const STYLES = ["paragraph", "heading2", "heading3", "heading4"] as const;
type TextStyle = (typeof STYLES)[number];

function currentStyle(editor: Editor): TextStyle | null {
  const parent = editor.state.selection.$from.parent;
  if (parent.type.name === "paragraph") return "paragraph";
  if (parent.type.name === "heading") return `heading${parent.attrs.level}` as TextStyle;
  return null;
}

function setStyle(editor: Editor, style: TextStyle) {
  const chain = editor.chain().focus();
  if (style === "paragraph") chain.setParagraph().run();
  else chain.setHeading({ level: Number(style.slice(-1)) as 2 | 3 | 4 }).run();
}

export interface EditorToolbarProps {
  editor: Editor;
  /** Whether "Insert" has its menu open; the slash menu opens the same one. */
  isInsertOpen: boolean;
  onInsertOpenChange: (open: boolean) => void;
  onInsert: (kind: BlockKind) => void;
  /** Blocks that cannot be inserted here, such as an image without an upload route. */
  disabledBlocks: BlockKind[];
}

export function EditorToolbar({
  editor,
  isInsertOpen,
  onInsertOpenChange,
  onInsert,
  disabledBlocks,
}: EditorToolbarProps) {
  const { t } = useStepclimbStrings();
  const style = currentStyle(editor);
  // Headings are top-level blocks only: inside a list, a quote, a callout or a cell, the style
  // stays "Paragraph".
  const nested = editor.state.selection.$from.depth > 1;
  return (
    <Toolbar data-slot="editor-toolbar" aria-label={t("editor.toolbar")} className={styles.root()}>
      <MenuTrigger>
        <Button
          intent="ghost"
          size="sm"
          aria-label={t("editor.styleCurrent", { kind: style ?? "other" })}
          isDisabled={style === null || !editor.isEditable}
          className={styles.style()}
        >
          <span className={styles.styleValue()}>
            {t("editor.block", { kind: style ?? "other" })}
          </span>
          <Icon icon={CaretDownIcon} size="sm" />
        </Button>
        <Menu
          autoFocus="first"
          selectionMode="single"
          selectedKeys={style === null ? [] : [style]}
          disabledKeys={nested ? ["heading2", "heading3", "heading4"] : []}
          onAction={(key) => setStyle(editor, key as TextStyle)}
        >
          {STYLES.map((id) => (
            <MenuItem key={id} id={id}>
              {t("editor.block", { kind: id })}
            </MenuItem>
          ))}
        </Menu>
      </MenuTrigger>
      <Separator orientation="vertical" />
      {MARKS.map(([name, icon]) => (
        <ToggleButton
          key={name}
          intent="ghost"
          size="sm"
          aria-label={t("editor.mark", { mark: name })}
          isSelected={editor.isActive(name)}
          isDisabled={!editor.can().toggleMark(name)}
          onChange={() => editor.chain().focus().toggleMark(name).run()}
        >
          <Icon icon={icon} size="md" />
        </ToggleButton>
      ))}
      <LinkButton editor={editor} />
      <Separator orientation="vertical" />
      <ToggleButton
        intent="ghost"
        size="sm"
        aria-label={t("editor.block", { kind: "bulletList" })}
        isSelected={editor.isActive("bulletList")}
        isDisabled={!editor.can().toggleBulletList()}
        onChange={() => editor.chain().focus().toggleBulletList().run()}
      >
        <Icon icon={ListBulletsIcon} size="md" />
      </ToggleButton>
      <ToggleButton
        intent="ghost"
        size="sm"
        aria-label={t("editor.block", { kind: "orderedList" })}
        isSelected={editor.isActive("orderedList")}
        isDisabled={!editor.can().toggleOrderedList()}
        onChange={() => editor.chain().focus().toggleOrderedList().run()}
      >
        <Icon icon={ListNumbersIcon} size="md" />
      </ToggleButton>
      <Separator orientation="vertical" />
      <MenuTrigger isOpen={isInsertOpen} onOpenChange={onInsertOpenChange}>
        <Button intent="ghost" size="sm" isDisabled={!editor.isEditable}>
          <Icon icon={PlusIcon} size="md" />
          {t("editor.insert")}
        </Button>
        <Menu
          autoFocus="first"
          disabledKeys={disabledBlocks}
          onAction={(key) => onInsert(key as BlockKind)}
        >
          {blockKinds.map((kind) => (
            <MenuItem key={kind} id={kind}>
              {t("editor.block", { kind })}
            </MenuItem>
          ))}
        </Menu>
      </MenuTrigger>
      <Separator orientation="vertical" />
      <IconButton
        size="sm"
        icon={ArrowCounterClockwiseIcon}
        label={t("editor.undo")}
        isDisabled={!editor.can().undo()}
        onPress={() => editor.chain().focus().undo().run()}
      />
      <IconButton
        size="sm"
        icon={ArrowClockwiseIcon}
        label={t("editor.redo")}
        isDisabled={!editor.can().redo()}
        onPress={() => editor.chain().focus().redo().run()}
      />
    </Toolbar>
  );
}

/** "Link": a popover with the link's address, checked as the server checks it (B9). */
function LinkButton({ editor }: { editor: Editor }) {
  const { t } = useStepclimbStrings();
  const [open, setOpen] = useState(false);
  const [href, setHref] = useState("");
  const [tried, setTried] = useState(false);
  const active = editor.isActive("link");
  const stored = linkTarget(href.trim());

  const openChange = (next: boolean) => {
    if (next) {
      setHref(String(editor.getAttributes("link").href ?? ""));
      setTried(false);
    }
    setOpen(next);
  };
  const apply = (event: FormEvent) => {
    event.preventDefault();
    setTried(true);
    if (stored === null) return;
    const chain = editor.chain().focus().extendMarkRange("link");
    if (editor.state.selection.empty && !active) {
      chain.insertContent({
        type: "text",
        text: stored,
        marks: [{ type: "link", attrs: { href: stored } }],
      });
    } else {
      chain.setMark("link", { href: stored });
    }
    chain.run();
    setOpen(false);
  };
  const remove = () => {
    editor.chain().focus().extendMarkRange("link").unsetMark("link").run();
    setOpen(false);
  };

  return (
    <PopoverTrigger isOpen={open} onOpenChange={openChange}>
      <ToggleButton
        intent="ghost"
        size="sm"
        aria-label={t("editor.mark", { mark: "link" })}
        isSelected={active || open}
        isDisabled={!editor.isEditable || !editor.can().setMark("link")}
      >
        <Icon icon={LinkIcon} size="md" />
      </ToggleButton>
      <Popover placement="bottom start">
        <PopoverDialog aria-label={t("editor.link.title")}>
          <form data-slot="editor-link-form" className={styles.linkForm()} onSubmit={apply}>
            <TextField
              validationBehavior="aria"
              label={t("editor.link.address")}
              description={t("editor.link.description")}
              type="url"
              inputMode="url"
              autoFocus
              value={href}
              onChange={setHref}
              isInvalid={tried && stored === null}
              errorMessage={t("editor.link.invalid")}
            />
            <div className={styles.linkActions()}>
              {active ? (
                <Button intent="secondary" size="sm" onPress={remove}>
                  {t("editor.link.remove")}
                </Button>
              ) : null}
              <Button intent="primary" size="sm" type="submit">
                {t("editor.link.apply")}
              </Button>
            </div>
          </form>
        </PopoverDialog>
      </Popover>
    </PopoverTrigger>
  );
}
