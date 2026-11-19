// SPDX-License-Identifier: AGPL-3.0-or-later
// TagField: a labeled list of short words or phrases, such as a course's keywords, typed into a
// text input and shown as removable tags (docs/specs/m1/16-learning-ui-components.md B6). Built
// from React Aria's TextField for the entry and its TagGroup for the tags: Enter or the "Add"
// button adds the typed tag, and each tag's remove button, Backspace or Delete removes it. The
// arrow keys move between tags; the tag list is one tab stop.
import { PlusIcon, XIcon } from "@phosphor-icons/react";
import { useId, useRef, useState } from "react";
import {
  TextField as AriaTextField,
  Button,
  Input,
  type Key,
  Tag,
  TagGroup,
  TagList,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { button } from "./button";
import { FieldDescription, FieldError, FieldLabel, type FieldPartsProps, field } from "./field";

const parts = field();

export const tagField = tv({
  slots: {
    root: parts.root(),
    entry: "flex flex-col gap-1.5",
    row: "flex items-stretch gap-2",
    input: [parts.control(), parts.input(), "flex-1"],
    add: button({ intent: "secondary" }),
    list: "flex flex-wrap gap-2 outline-none",
    tag: [
      "inline-flex min-h-(--sc-control-h-sm) max-w-full cursor-default items-center gap-1 rounded-pill",
      "border border-line-strong bg-raised ps-3 pe-1 text-sm text-fg-default outline-none",
      "transition-colors duration-fast ease-out-expo data-[hovered]:border-fg-sub",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring data-[focus-visible]:ring-offset-2 ring-offset-canvas",
      "data-[disabled]:border-line data-[disabled]:pe-3 data-[disabled]:text-fg-disabled",
    ],
    // The tag's text wraps rather than overflowing a narrow screen.
    tagText: "min-w-0 break-words py-0.5",
    remove: [
      "flex size-6 shrink-0 items-center justify-center rounded-pill text-fg-sub outline-none",
      "transition-colors duration-fast ease-out-expo",
      "data-[hovered]:bg-sunken data-[hovered]:text-fg-strong data-[pressed]:bg-sunken",
      "data-[focus-visible]:ring-2 data-[focus-visible]:ring-ring",
    ],
  },
});

export interface TagFieldProps extends FieldPartsProps {
  /** Controlled: the tags, in the order they were added. */
  value?: readonly string[];
  /** Uncontrolled: the tags it starts with. */
  defaultValue?: readonly string[];
  /** Called with the whole list after a tag is added or removed. */
  onChange?: (tags: string[]) => void;
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** Without a visible `label`, names the input and the tag list. */
  "aria-label"?: string;
  isDisabled?: boolean;
  /** Draws the error state and shows `errorMessage`. The caller decides what is valid. */
  isInvalid?: boolean;
  /** Layout only. */
  className?: string;
}

/** Tags held here unless `value` controls them. */
function useTags({ value, defaultValue = [], onChange }: TagFieldProps) {
  const [own, setOwn] = useState<readonly string[]>(defaultValue);
  const tags = value ?? own;
  const setTags = (next: string[]) => {
    if (value === undefined) setOwn(next);
    onChange?.(next);
  };
  return [tags, setTags] as const;
}

/**
 * `text` trimmed, or null when it adds nothing: it is blank or the list already has it, compared
 * without regard to case.
 */
export function newTag(tags: readonly string[], text: string): string | null {
  const tag = text.trim();
  if (tag === "") return null;
  const lower = tag.toLocaleLowerCase();
  return tags.some((t) => t.toLocaleLowerCase() === lower) ? null : tag;
}

/**
 * Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. Enter with text in the
 * input adds it instead of submitting the form; a blank or repeated tag is not added.
 */
export function TagField(props: TagFieldProps) {
  const { label, description, errorMessage, placeholder, isDisabled, isInvalid, className } = props;
  const { t } = useStepclimbStrings();
  const styles = tagField();
  const [tags, setTags] = useTags(props);
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const labelId = useId();
  const inputId = useId();
  const named =
    label === undefined ? { "aria-label": props["aria-label"] } : { "aria-labelledby": labelId };

  const add = () => {
    const tag = newTag(tags, draft);
    setDraft("");
    if (tag !== null) setTags([...tags, tag]);
  };
  const remove = (keys: Set<Key>) => {
    const next = tags.filter((tag) => !keys.has(tag));
    setTags(next);
    // The last tag's removal takes the list away; focus goes back to the input.
    if (next.length === 0) inputRef.current?.focus();
  };

  return (
    <div data-slot="tag-field" className={styles.root({ className })}>
      {label === undefined ? null : (
        <FieldLabel id={labelId} htmlFor={inputId}>
          {label}
        </FieldLabel>
      )}
      <AriaTextField
        data-slot="tag-field-entry"
        id={inputId}
        {...named}
        value={draft}
        onChange={setDraft}
        onKeyDown={(event) => {
          if (event.key !== "Enter" || draft.trim() === "") return;
          event.preventDefault();
          add();
        }}
        isDisabled={isDisabled}
        isInvalid={isInvalid}
        validationBehavior="aria"
        className={styles.entry()}
      >
        <div data-slot="tag-field-row" className={styles.row()}>
          <Input
            ref={inputRef}
            data-slot="tag-field-input"
            placeholder={placeholder}
            className={styles.input()}
          />
          <Button
            data-slot="tag-field-add"
            onPress={add}
            isDisabled={isDisabled || draft.trim() === ""}
            className={styles.add()}
          >
            <Icon icon={PlusIcon} size="sm" />
            {t("tagField.add")}
          </Button>
        </div>
        {description === undefined ? null : <FieldDescription>{description}</FieldDescription>}
        <FieldError>{errorMessage}</FieldError>
      </AriaTextField>
      {tags.length === 0 ? null : (
        <TagGroup
          data-slot="tag-field-tags"
          {...named}
          onRemove={isDisabled ? undefined : remove}
          disabledKeys={isDisabled ? tags : undefined}
        >
          <TagList data-slot="tag-field-list" className={styles.list()}>
            {tags.map((tag) => (
              <Tag
                key={tag}
                id={tag}
                textValue={tag}
                data-slot="tag-field-tag"
                className={styles.tag()}
              >
                {({ allowsRemoving }) => (
                  <>
                    <span data-slot="tag-field-tag-text" className={styles.tagText()}>
                      {tag}
                    </span>
                    {allowsRemoving ? (
                      <Button
                        slot="remove"
                        data-slot="tag-field-remove"
                        aria-label={t("tagField.remove")}
                        className={styles.remove()}
                      >
                        <Icon icon={XIcon} size="sm" />
                      </Button>
                    ) : null}
                  </>
                )}
              </Tag>
            ))}
          </TagList>
        </TagGroup>
      )}
    </div>
  );
}
