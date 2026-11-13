// SPDX-License-Identifier: AGPL-3.0-or-later
// ComboBox: a text field that filters a list of options as the person types
// (docs/specs/m0/13-base-ui-components.md M0 inventory). At 768 px and wider the options open in a
// Popover under the input. Under 768 px (Q10) the field is a button that opens a bottom sheet
// holding the input and the options, so the list sits with the input above the phone's keyboard
// instead of in a popover the keyboard covers. With `isLoading` it shows a spinner, and an empty
// list says it is loading rather than that nothing matched.
import { CaretDownIcon, CircleNotchIcon } from "@phosphor-icons/react";
import { type ReactNode, use, useId } from "react";
import {
  ComboBox as AriaComboBox,
  type ComboBoxProps as AriaComboBoxProps,
  Button,
  ComboBoxStateContext,
  Group,
  Input,
  LabelContext,
  ProgressBar,
} from "react-aria-components";
import { WIDE_QUERY } from "../lib/breakpoints";
import { tv } from "../lib/tv";
import { useMediaQuery } from "../lib/use-media-query";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";
import { FieldParts, type FieldPartsProps, field } from "./field";
import {
  type ListBoxChildren,
  ListBoxItem,
  type ListBoxItemProps,
  ListBoxOverlay,
} from "./list-box";

const parts = field();

export const comboBox = tv({
  slots: {
    root: parts.root(),
    group: [parts.control(), "flex h-(--sc-control-h) items-center"],
    // The group draws the focus ring around the input and its button together.
    input:
      "h-full min-w-0 flex-1 bg-transparent ps-3 outline-none focus-visible:shadow-none placeholder:text-fg-soft",
    button: "flex h-full shrink-0 items-center px-2 text-fg-sub outline-none cursor-default",
    // No color transition: the trigger changes state as the sheet opens over it and closes, where
    // no one sees it, and with a transition its screenshot under the sheet differed by a few
    // pixels from one run to the next.
    sheetTrigger: [
      parts.control(),
      "flex h-(--sc-control-h) items-center gap-2 px-3 text-start cursor-default transition-none",
    ],
    sheetValue: "min-w-0 flex-1 truncate",
    sheetPlaceholder: "min-w-0 flex-1 truncate text-fg-sub",
    sheetInput: [parts.control(), parts.input()],
    sheetHeader: "px-4 pt-4 pb-2",
    caret: "shrink-0 text-fg-sub",
    spinner: "flex shrink-0 items-center text-fg-sub",
    empty: "px-3 py-2 text-sm text-fg-sub",
  },
});

const styles = comboBox();

export interface ComboBoxProps<T extends object>
  extends Omit<
      AriaComboBoxProps<T>,
      "className" | "children" | "allowsEmptyCollection" | "selectionMode"
    >,
    FieldPartsProps {
  /** The options: `ComboBoxItem`s, or a function that renders each of `items`. */
  children: ListBoxChildren<T>;
  /** Shown in the empty input. Never instead of a label. */
  placeholder?: string;
  /** The options are loading: a spinner in the field, and an empty list says so. */
  isLoading?: boolean;
  /** Layout only. */
  className?: string;
}

/**
 * Controlled with `value` and `onChange` (the chosen item's id) and `inputValue` and
 * `onInputChange` (the text); uncontrolled with `defaultValue` and `defaultInputValue`. Static
 * `ComboBoxItem` children are filtered as the person types; with `items`, the caller filters
 * (for options loaded from the server) and sets `isLoading` while it waits.
 */
export function ComboBox<T extends object>({
  label,
  description,
  errorMessage,
  placeholder,
  isLoading = false,
  className,
  children,
  ...props
}: ComboBoxProps<T>) {
  const { t } = useStepclimbStrings();
  const wide = useMediaQuery(WIDE_QUERY);
  const spinner = isLoading ? (
    <ProgressBar
      data-slot="combo-box-loading"
      isIndeterminate
      aria-label={t("comboBox.loading")}
      className={styles.spinner()}
    >
      <Icon icon={CircleNotchIcon} size="sm" className="animate-spin" />
    </ProgressBar>
  ) : null;
  const renderEmptyState = () => (
    <div data-slot="combo-box-empty" className={styles.empty()}>
      {isLoading ? t("comboBox.loading") : t("comboBox.empty")}
    </div>
  );
  const input = (sheet: boolean) => (
    <Input
      data-slot="combo-box-input"
      placeholder={placeholder}
      // The sheet opens for typing: its input takes focus, which brings up the keyboard.
      autoFocus={sheet}
      className={sheet ? styles.sheetInput() : styles.input()}
    />
  );
  return (
    <AriaComboBox<T>
      data-slot="combo-box"
      allowsEmptyCollection
      {...props}
      className={styles.root({ className })}
    >
      <FieldParts label={label} description={description} errorMessage={errorMessage}>
        {wide ? (
          <Group data-slot="combo-box-group" className={styles.group()}>
            {input(false)}
            {spinner}
            <Button
              data-slot="combo-box-button"
              aria-label={t("comboBox.showSuggestions")}
              className={styles.button()}
            >
              <Icon icon={CaretDownIcon} size="sm" />
            </Button>
          </Group>
        ) : (
          <SheetTrigger placeholder={placeholder}>{spinner}</SheetTrigger>
        )}
      </FieldParts>
      <ListBoxOverlay<T>
        renderEmptyState={renderEmptyState}
        sheetHeader={<div className={styles.sheetHeader()}>{input(true)}</div>}
      >
        {children}
      </ListBoxOverlay>
    </AriaComboBox>
  );
}

/**
 * The field under 768 px: a button showing the input's text, or the placeholder while it is
 * empty. It is named by the field's label and that text, so what it shows is in its name.
 */
function SheetTrigger({ placeholder, children }: { placeholder?: string; children: ReactNode }) {
  const text = use(ComboBoxStateContext)?.inputValue ?? "";
  const label = use(LabelContext);
  const labelId = label && "id" in label ? label.id : undefined;
  const valueId = useId();
  const { t } = useStepclimbStrings();
  return (
    <Button
      data-slot="combo-box-sheet-trigger"
      // React Aria labels this button with its own "Show suggestions" translation; the strings
      // catalog's replaces it, though the label and value below are what name the button.
      aria-label={t("comboBox.showSuggestions")}
      aria-labelledby={[labelId, valueId].filter(Boolean).join(" ")}
      className={styles.sheetTrigger()}
    >
      <span
        id={valueId}
        data-slot="combo-box-value"
        data-placeholder={text === "" || undefined}
        className={text === "" ? styles.sheetPlaceholder() : styles.sheetValue()}
      >
        {text === "" ? placeholder : text}
      </span>
      {children}
      <Icon icon={CaretDownIcon} size="sm" className={styles.caret()} />
    </Button>
  );
}

/** One option of a ComboBox. */
export function ComboBoxItem<T extends object>(props: ListBoxItemProps<T>) {
  return <ListBoxItem {...props} />;
}
