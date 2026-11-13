// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI: the Foundations, the Actions, the data states, the text fields, Popover and Tooltip,
// and the theme and strings providers so far; the rest of the M0 inventory is specified in
// docs/specs/m0/13-base-ui-components.md.
export { EmptyState, type EmptyStateProps, emptyState } from "./patterns/empty-state";
export { ErrorState, type ErrorStateProps, errorState } from "./patterns/error-state";
export { Avatar, type AvatarProps, avatar, initials } from "./primitives/avatar";
export { Button, type ButtonProps, button } from "./primitives/button";
export { ButtonGroup, type ButtonGroupProps, buttonGroup } from "./primitives/button-group";
export {
  CopyButton,
  type CopyButtonProps,
  type CopyResult,
  copyButton,
} from "./primitives/copy-button";
export {
  FieldDescription,
  type FieldDescriptionProps,
  FieldError,
  type FieldErrorMessage,
  type FieldErrorProps,
  FieldLabel,
  type FieldLabelProps,
  type FieldPartsProps,
  field,
} from "./primitives/field";
export {
  Heading,
  type HeadingLevel,
  type HeadingProps,
  type HeadingSize,
  heading,
  headingSizes,
} from "./primitives/heading";
export { IconButton, type IconButtonProps, iconButton } from "./primitives/icon-button";
export { Kbd, type KbdProps, kbd } from "./primitives/kbd";
export { isExternalHref, Link, type LinkProps, link } from "./primitives/link";
export {
  MENU_POPOVER_QUERY,
  Menu,
  MenuItem,
  type MenuItemProps,
  type MenuProps,
  MenuTrigger,
  menu,
  SubmenuTrigger,
} from "./primitives/menu";
export { Meter, type MeterProps, meter } from "./primitives/meter";
export { NumberField, type NumberFieldProps, numberField } from "./primitives/number-field";
export {
  Popover,
  PopoverDialog,
  type PopoverDialogProps,
  type PopoverProps,
  PopoverTrigger,
  popover,
} from "./primitives/popover";
export {
  fractionOf,
  ProgressBar,
  type ProgressBarProps,
  progressBar,
} from "./primitives/progress-bar";
export { Prose, type ProseProps, prose } from "./primitives/prose";
export { SearchField, type SearchFieldProps, searchField } from "./primitives/search-field";
export { Separator, type SeparatorProps, separator } from "./primitives/separator";
export { SKELETON_DELAY_MS, Skeleton, type SkeletonProps, skeleton } from "./primitives/skeleton";
export { Text, type TextProps, text } from "./primitives/text";
export { TextArea, type TextAreaProps, textArea } from "./primitives/text-area";
export { TextField, type TextFieldProps, textField } from "./primitives/text-field";
export {
  ToggleButton,
  ToggleButtonGroup,
  type ToggleButtonGroupProps,
  type ToggleButtonProps,
  toggleButton,
  toggleButtonGroup,
} from "./primitives/toggle-button";
export { Toolbar, type ToolbarProps, toolbar } from "./primitives/toolbar";
export { Tooltip, type TooltipProps, TooltipTrigger, tooltip } from "./primitives/tooltip";
export { VisuallyHidden, type VisuallyHiddenProps } from "./primitives/visually-hidden";
export { type Catalog, type Messages, uiMessages } from "./strings/catalog";
export {
  createStrings,
  type MessageValues,
  type Strings,
  type StringsOptions,
  StringsProvider,
  type StringsProviderProps,
  useStepclimbStrings,
} from "./strings/strings";
export { defaultTerms, type Term, type TermForms, type TermOverrides } from "./strings/terms";
export { Icon, type IconProps, type IconState, icon, iconWeights } from "./theme/icon";
export {
  type Appearance,
  type Density,
  densities,
  type Theme,
  ThemeProvider,
  type ThemeProviderProps,
  themes,
  useAppearance,
} from "./theme/theme-provider";
