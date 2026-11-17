// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI: the M0 inventory of docs/specs/m0/13-base-ui-components.md (Foundations, Actions,
// the M0 forms, Overlays, Navigation and the M0 data components), the M1 components of
// docs/specs/m1/16-learning-ui-components.md built so far, and the theme and strings providers.
export { DueBadge, type DueBadgeProps, dueBadge, dueDays } from "./learning/due-badge";
export type { LockReason } from "./learning/lock-reason";
export {
  ProgressRail,
  type ProgressRailItem,
  type ProgressRailProps,
  progressRail,
} from "./learning/progress-rail";
export {
  type Requirement,
  RequirementLabel,
  type RequirementLabelProps,
  requirementLabel,
} from "./learning/requirement-label";
export {
  type LearningState,
  learningStateIcons,
  learningStates,
  StatusChip,
  type StatusChipProps,
  statusChip,
} from "./learning/status-chip";
export { AppShell, type AppShellProps, appShell } from "./patterns/app-shell";
export { BottomTabs, type BottomTabsProps, bottomTabs } from "./patterns/bottom-tabs";
export {
  type Command,
  CommandPalette,
  type CommandPaletteProps,
  commandPalette,
  isPaletteShortcut,
} from "./patterns/command-palette";
export {
  DataTable,
  type DataTableColumn,
  type DataTableProps,
  dataTable,
} from "./patterns/data-table";
export { EmptyState, type EmptyStateProps, emptyState } from "./patterns/empty-state";
export { ErrorState, type ErrorStateProps, errorState } from "./patterns/error-state";
export {
  acceptsType,
  checkFiles,
  FileDrop,
  type FileDropProps,
  type FileRejection,
  type FileUpload,
  fileDrop,
} from "./patterns/file-drop";
export { FormError, type FormErrorProps, formError } from "./patterns/form-error";
export {
  type Organization,
  OrgSwitcher,
  type OrgSwitcherProps,
  orgSwitcher,
} from "./patterns/org-switcher";
export { type NavEntry, SideNav, type SideNavProps, sideNav } from "./patterns/side-nav";
export { TopBar, type TopBarProps, topBar } from "./patterns/top-bar";
export { AlertDialog, type AlertDialogProps } from "./primitives/alert-dialog";
export { Avatar, type AvatarProps, avatar, initials } from "./primitives/avatar";
export {
  Breadcrumb,
  type BreadcrumbProps,
  Breadcrumbs,
  type BreadcrumbsProps,
  breadcrumbs,
} from "./primitives/breadcrumbs";
export { Button, type ButtonProps, button } from "./primitives/button";
export { ButtonGroup, type ButtonGroupProps, buttonGroup } from "./primitives/button-group";
export { ButtonLink, type ButtonLinkProps, buttonLink } from "./primitives/button-link";
export { calendar } from "./primitives/calendar";
export {
  Checkbox,
  CheckboxGroup,
  type CheckboxGroupProps,
  type CheckboxProps,
  checkbox,
} from "./primitives/checkbox";
export { Code, type CodeProps, code } from "./primitives/code";
export {
  ComboBox,
  ComboBoxItem,
  type ComboBoxProps,
  comboBox,
} from "./primitives/combo-box";
export {
  CopyButton,
  type CopyButtonProps,
  type CopyResult,
  copyButton,
} from "./primitives/copy-button";
export {
  DataList,
  DataListItem,
  type DataListItemProps,
  type DataListProps,
  dataList,
} from "./primitives/data-list";
export { dateInput } from "./primitives/date-input";
export { DatePicker, type DatePickerProps, datePicker } from "./primitives/date-picker";
export {
  DateRangePicker,
  type DateRangePickerProps,
  dateRangePicker,
} from "./primitives/date-range-picker";
export {
  Dialog,
  DialogBody,
  type DialogChildren,
  DialogFooter,
  type DialogPartProps,
  type DialogProps,
  DialogTrigger,
  dialog,
} from "./primitives/dialog";
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
export { type ListBoxItemProps, listBox } from "./primitives/list-box";
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
export { Pagination, type PaginationProps, pagination } from "./primitives/pagination";
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
export {
  Radio,
  RadioGroup,
  type RadioGroupProps,
  type RadioProps,
  radioGroup,
} from "./primitives/radio-group";
export { SearchField, type SearchFieldProps, searchField } from "./primitives/search-field";
export { Select, SelectItem, type SelectProps, select } from "./primitives/select";
export { Separator, type SeparatorProps, separator } from "./primitives/separator";
export { Sheet, type SheetPlacement, type SheetProps, sheet } from "./primitives/sheet";
export { SKELETON_DELAY_MS, Skeleton, type SkeletonProps, skeleton } from "./primitives/skeleton";
export { Switch, type SwitchProps, switchField } from "./primitives/switch";
export {
  Cell,
  type CellProps,
  Column,
  type ColumnProps,
  Row,
  type RowProps,
  Table,
  TableBody,
  type TableBodyProps,
  TableHeader,
  type TableHeaderProps,
  type TableProps,
  table,
} from "./primitives/table";
export {
  Tab,
  TabList,
  type TabListProps,
  TabPanel,
  type TabPanelProps,
  type TabProps,
  Tabs,
  type TabsProps,
  tabs,
} from "./primitives/tabs";
export { Text, type TextProps, text } from "./primitives/text";
export { TextArea, type TextAreaProps, textArea } from "./primitives/text-area";
export { TextField, type TextFieldProps, textField } from "./primitives/text-field";
export { TimeField, type TimeFieldProps, timeField } from "./primitives/time-field";
export {
  createToastQueue,
  TOAST_MIN_TIMEOUT_MS,
  TOAST_TIMEOUT_MS,
  type ToastMessage,
  type ToastOptions,
  type ToastQueue,
  ToastRegion,
  type ToastRegionProps,
  type ToastTone,
  toast,
  toastTimeout,
} from "./primitives/toast";
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
export { Tree, TreeItem, type TreeItemProps, type TreeProps, tree } from "./primitives/tree";
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
