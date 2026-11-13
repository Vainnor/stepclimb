// SPDX-License-Identifier: AGPL-3.0-or-later
// ComboBox: a text field that filters a list of options as the person types
// (docs/specs/m0/13-base-ui-components.md M0 inventory). At 768 px and wider the options open in a
// Popover under the input. Under 768 px (Q10) the field is a button that opens a bottom sheet
// holding the input and the options; a sheet that left the input in the page would trap focus
// away from it. With `isLoading` it shows a spinner, and an empty list says it is loading rather
// than that nothing matched.
import { CaretDownIcon, CircleNotchIcon } from "@phosphor-icons/react";
import { type ReactNode, use, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import {
  ComboBox as AriaComboBox,
  type ComboBoxProps as AriaComboBoxProps,
  Button,
  ComboBoxStateContext,
  FieldErrorContext,
  Group,
  Input,
  LabelContext,
  ProgressBar,
  TextContext,
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
import { SKELETON_DELAY_MS } from "./skeleton";

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
    formInput: "sr-only",
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
  const spinner = isLoading ? <LoadingSpinner /> : null;
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
          <PhoneField placeholder={placeholder}>{spinner}</PhoneField>
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
 * The loading spinner, shown once the wait has lasted SKELETON_DELAY_MS (300 ms), like Button's,
 * so a fast answer never flashes it. The empty list says "Loading" from the start.
 */
function LoadingSpinner() {
  const { t } = useStepclimbStrings();
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);
  if (!shown) return null;
  return (
    <ProgressBar
      data-slot="combo-box-loading"
      isIndeterminate
      aria-label={t("comboBox.loading")}
      className={styles.spinner()}
    >
      <Icon icon={CircleNotchIcon} size="sm" className="animate-spin" />
    </ProgressBar>
  );
}

/** The `id` React Aria gives a slot of the field's Text context: its description or error. */
function useTextSlotId(slot: "description" | "errorMessage"): string | undefined {
  const context = use(TextContext);
  if (!context || !("slots" in context) || !context.slots) return undefined;
  const props = (context.slots as Record<string, { id?: string } | undefined>)[slot];
  return props?.id;
}

/**
 * The field under 768 px: a button showing the input's text, or the placeholder while it is
 * empty, named by the field's label and that text so what it shows is in its name. It carries the
 * field's description, error and invalid state, which React Aria puts on the input.
 *
 * While the sheet is closed the input stays in the page, visually hidden and out of the tab order,
 * so the form still submits its `name` and the browser still checks `isRequired`. When the label
 * is pressed, or the browser focuses the input to report it invalid, focus moves to the button.
 */
function PhoneField({ placeholder, children }: { placeholder?: string; children: ReactNode }) {
  const { t } = useStepclimbStrings();
  const state = use(ComboBoxStateContext);
  const validation = use(FieldErrorContext);
  const label = use(LabelContext);
  const labelId = label && "id" in label ? label.id : undefined;
  const descriptionId = useTextSlotId("description");
  const errorId = useTextSlotId("errorMessage");
  const isInvalid = validation?.isInvalid ?? false;
  const valueId = useId();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const text = state?.inputValue ?? "";
  // React Aria's Button passes only labeling ARIA props to the DOM, so the invalid state is set
  // here. `aria-required` is not allowed on a button; the required state shows through the
  // browser's check of the hidden input and the error message.
  useLayoutEffect(() => {
    const button = buttonRef.current;
    if (!button) return;
    if (isInvalid) button.setAttribute("aria-invalid", "true");
    else button.removeAttribute("aria-invalid");
  }, [isInvalid]);
  return (
    <>
      <Button
        ref={buttonRef}
        data-slot="combo-box-sheet-trigger"
        // React Aria labels this button with its own "Show suggestions" translation; the strings
        // catalog's replaces it, though the label and value below are what name the button.
        aria-label={t("comboBox.showSuggestions")}
        aria-labelledby={[labelId, valueId].filter(Boolean).join(" ")}
        aria-describedby={
          [descriptionId, isInvalid ? errorId : undefined].filter(Boolean).join(" ") || undefined
        }
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
      {state?.isOpen ? null : (
        <Input
          data-slot="combo-box-form-input"
          aria-hidden
          tabIndex={-1}
          onFocus={() => buttonRef.current?.focus()}
          className={styles.formInput()}
        />
      )}
    </>
  );
}

/** One option of a ComboBox. */
export function ComboBoxItem<T extends object>(props: ListBoxItemProps<T>) {
  return <ListBoxItem {...props} />;
}
