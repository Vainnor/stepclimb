// SPDX-License-Identifier: AGPL-3.0-or-later
// SearchField: a labeled search input with a clear button, labeled from the strings catalog, that
// shows while there is text (docs/specs/m0/13-base-ui-components.md M0 inventory). Escape clears
// it too; Enter calls `onSubmit`.
import { MagnifyingGlassIcon, XIcon } from "@phosphor-icons/react";
import {
  SearchField as AriaSearchField,
  type SearchFieldProps as AriaSearchFieldProps,
  Button,
  Input,
} from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { FieldParts, type FieldPartsProps, field } from "./field";

const parts = field();

export const searchField = tv({
  slots: {
    root: [parts.root(), "group"],
    box: "relative flex items-center",
    icon: "pointer-events-none absolute start-3 text-fg-soft",
    input: [parts.control(), parts.input(), "ps-9 pe-11 [&::-webkit-search-cancel-button]:hidden"],
    clear: [
      "absolute end-0 inset-y-0 flex aspect-square items-center justify-center rounded-sm text-fg-sub",
      "outline-none transition-colors duration-fast ease-out-expo",
      "data-[hovered]:text-fg-strong data-[pressed]:text-fg-strong group-data-[empty]:hidden",
    ],
  },
});

export interface SearchFieldProps
  extends Omit<AriaSearchFieldProps, "className" | "children">,
    FieldPartsProps {
  /** Shown while the input is empty. Never instead of a label. */
  placeholder?: string;
  /** Layout only. */
  className?: string;
}

/** Controlled with `value` and `onChange`, uncontrolled with `defaultValue`. */
export function SearchField({
  label,
  description,
  errorMessage,
  placeholder,
  className,
  ...props
}: SearchFieldProps) {
  const { t } = useStepclimbStrings();
  const styles = searchField();
  return (
    <AriaSearchField data-slot="search-field" {...props} className={styles.root({ className })}>
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        <div data-slot="search-field-box" className={styles.box()}>
          <Icon icon={MagnifyingGlassIcon} size="sm" className={styles.icon()} />
          <Input
            data-slot="search-field-input"
            placeholder={placeholder}
            className={styles.input()}
          />
          <Button
            data-slot="search-field-clear"
            aria-label={t("searchField.clear")}
            className={styles.clear()}
          >
            <Icon icon={XIcon} size="sm" />
          </Button>
        </div>
      </FieldParts>
    </AriaSearchField>
  );
}
