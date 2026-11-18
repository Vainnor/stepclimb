// SPDX-License-Identifier: AGPL-3.0-or-later
// The block toolbar (docs/specs/m1/07-pages-and-the-editor.md B21): the settings of the block
// holding the selection (a code block's language, a callout's kind, an image's text alternative,
// an embed's title, a transcript line's speaker, a table's rows and columns, a list item's level)
// and moving or deleting the top-level block. It sits under the main toolbar rather than floating
// beside the block, so it stays reachable with the keyboard and on a phone's narrow screen.
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ColumnsPlusRightIcon,
  RowsPlusBottomIcon,
  TextIndentIcon,
  TextOutdentIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import type { Editor } from "@tiptap/core";
import type { ReactNode } from "react";
import { calloutKinds } from "../learning/callout";
import { tv } from "../lib/tv";
import { Button } from "../primitives/button";
import { Checkbox } from "../primitives/checkbox";
import { IconButton } from "../primitives/icon-button";
import { Select, SelectItem } from "../primitives/select";
import { TextField } from "../primitives/text-field";
import { useStepclimbStrings } from "../strings/strings";
import { type BlockContext, deleteBlock, moveBlock, setAttrsAt } from "./blocks";
import { codeLanguages, MAX_EMBED_TITLE_CHARS, MAX_SPEAKER_CHARS } from "./page-doc";

export const blockToolbar = tv({
  slots: {
    root: "m-0 flex min-w-0 flex-wrap items-end gap-x-3 gap-y-2 border-0 border-t border-line-soft px-3 py-2",
    name: "me-auto self-center font-ui text-sm font-medium text-fg-strong",
    settings: "flex min-w-0 flex-wrap items-end gap-2",
    field: "w-56 max-w-full",
    actions: "flex flex-wrap items-center gap-1",
  },
});

const styles = blockToolbar();

export interface BlockToolbarProps {
  editor: Editor;
  context: BlockContext;
  /** The top-level block count, so "Move down" stops at the end. */
  count: number;
}

/** What the block toolbar calls the block, from its node. */
function blockName(context: BlockContext): string {
  const top = context.top;
  if (top.type.name === "heading") return `heading${top.attrs.level}`;
  return top.type.name;
}

export function BlockToolbar({ editor, context, count }: BlockToolbarProps) {
  const { t } = useStepclimbStrings();
  const editable = editor.isEditable;
  return (
    <fieldset
      data-slot="block-toolbar"
      aria-label={t("editor.blockToolbar", { kind: blockName(context) })}
      className={styles.root()}
    >
      <span data-slot="block-toolbar-name" className={styles.name()}>
        {t("editor.block", { kind: blockName(context) })}
      </span>
      <div data-slot="block-toolbar-settings" className={styles.settings()}>
        <Settings editor={editor} context={context} />
      </div>
      <div data-slot="block-toolbar-actions" className={styles.actions()}>
        <IconButton
          size="sm"
          icon={ArrowUpIcon}
          label={t("editor.moveUp")}
          isDisabled={!editable || context.index === 0}
          onPress={() => moveBlock(editor, context.index, -1)}
        />
        <IconButton
          size="sm"
          icon={ArrowDownIcon}
          label={t("editor.moveDown")}
          isDisabled={!editable || context.index >= count - 1}
          onPress={() => moveBlock(editor, context.index, 1)}
        />
        <IconButton
          size="sm"
          icon={TrashIcon}
          label={t("editor.deleteBlock")}
          isDisabled={!editable}
          onPress={() => deleteBlock(editor, context.index)}
        />
      </div>
    </fieldset>
  );
}

/** The fields of the innermost block with settings. */
function Settings({ editor, context }: { editor: Editor; context: BlockContext }): ReactNode {
  const { node, pos } = context;
  const set = (attrs: Record<string, unknown>) => setAttrsAt(editor, pos, attrs);
  switch (node.type.name) {
    case "codeBlock":
      return (
        <LanguageField value={node.attrs.language} onChange={(language) => set({ language })} />
      );
    case "callout":
      return <KindField value={node.attrs.kind} onChange={(kind) => set({ kind })} />;
    case "image":
      return <ImageFields alt={node.attrs.alt} decorative={node.attrs.decorative} onChange={set} />;
    case "videoEmbed":
      return (
        <LabelField
          id="title"
          value={node.attrs.title}
          max={MAX_EMBED_TITLE_CHARS}
          onChange={(title) => set({ title })}
        />
      );
    case "transcriptLine":
      return (
        <LabelField
          id="speaker"
          value={node.attrs.speaker}
          max={MAX_SPEAKER_CHARS}
          onChange={(speaker) => set({ speaker })}
        />
      );
    case "tableCell":
    case "tableHeader":
      return <TableActions editor={editor} />;
    case "listItem":
      return <ListActions editor={editor} />;
    default:
      return null;
  }
}

function LanguageField({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
}) {
  const { t } = useStepclimbStrings();
  return (
    <Select
      label={t("editor.code.language")}
      value={value ?? "none"}
      onChange={(key) => onChange(key === "none" ? null : String(key))}
      className={styles.field()}
    >
      <SelectItem id="none">{t("editor.code.none")}</SelectItem>
      {codeLanguages.map((language) => (
        <SelectItem key={language} id={language}>
          {t("editor.code.languageName", { language })}
        </SelectItem>
      ))}
    </Select>
  );
}

function KindField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useStepclimbStrings();
  return (
    <Select
      label={t("editor.callout.kind")}
      value={value}
      onChange={(key) => onChange(String(key))}
      className={styles.field()}
    >
      {calloutKinds.map((kind) => (
        <SelectItem key={kind} id={kind}>
          {t("callout.kind", { kind })}
        </SelectItem>
      ))}
    </Select>
  );
}

interface ImageFieldsProps {
  alt: string;
  decorative: boolean;
  onChange: (attrs: Record<string, unknown>) => void;
}

/** The text alternative, required unless the image is decorative (B10). */
function ImageFields({ alt, decorative, onChange }: ImageFieldsProps) {
  const { t } = useStepclimbStrings();
  return (
    <>
      <TextField
        validationBehavior="aria"
        label={t("editor.image.alt")}
        value={alt}
        isDisabled={decorative}
        isRequired={!decorative}
        isInvalid={!decorative && alt.trim() === ""}
        errorMessage={t("editor.image.altMissing")}
        onChange={(value) => onChange({ alt: value })}
        className={styles.field()}
      />
      <Checkbox isSelected={decorative} onChange={(value) => onChange({ decorative: value })}>
        {t("editor.image.decorativeOption")}
      </Checkbox>
    </>
  );
}

interface LabelFieldProps {
  id: "title" | "speaker";
  value: string;
  max: number;
  onChange: (value: string) => void;
}

/** A required short label: an embed's title or a transcript line's speaker (B11, B12). */
function LabelField({ id, value, max, onChange }: LabelFieldProps) {
  const { t } = useStepclimbStrings();
  const blank = value.trim() === "";
  return (
    <TextField
      validationBehavior="aria"
      label={t("editor.label", { field: id })}
      value={value}
      isRequired
      maxLength={max}
      isInvalid={blank}
      errorMessage={t("editor.labelMissing", { field: id })}
      onChange={onChange}
      className={styles.field()}
    />
  );
}

function TableActions({ editor }: { editor: Editor }) {
  const { t } = useStepclimbStrings();
  const run = (command: (chain: ReturnType<Editor["chain"]>) => ReturnType<Editor["chain"]>) =>
    command(editor.chain().focus()).run();
  return (
    <>
      <IconButton
        size="sm"
        icon={RowsPlusBottomIcon}
        label={t("editor.table.addRow")}
        onPress={() => run((c) => c.addRowAfter())}
      />
      <IconButton
        size="sm"
        icon={ColumnsPlusRightIcon}
        label={t("editor.table.addColumn")}
        onPress={() => run((c) => c.addColumnAfter())}
      />
      <Button size="sm" intent="ghost" onPress={() => run((c) => c.deleteRow())}>
        {t("editor.table.deleteRow")}
      </Button>
      <Button size="sm" intent="ghost" onPress={() => run((c) => c.deleteColumn())}>
        {t("editor.table.deleteColumn")}
      </Button>
      <Button size="sm" intent="ghost" onPress={() => run((c) => c.toggleHeaderRow())}>
        {t("editor.table.headerRow")}
      </Button>
    </>
  );
}

function ListActions({ editor }: { editor: Editor }) {
  const { t } = useStepclimbStrings();
  return (
    <>
      <IconButton
        size="sm"
        icon={TextIndentIcon}
        label={t("editor.list.indent")}
        isDisabled={!editor.can().sinkListItem("listItem")}
        onPress={() => editor.chain().focus().sinkListItem("listItem").run()}
      />
      <IconButton
        size="sm"
        icon={TextOutdentIcon}
        label={t("editor.list.outdent")}
        isDisabled={!editor.can().liftListItem("listItem")}
        onPress={() => editor.chain().focus().liftListItem("listItem").run()}
      />
    </>
  );
}
