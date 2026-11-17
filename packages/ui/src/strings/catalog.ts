// SPDX-License-Identifier: AGPL-3.0-or-later
// Message catalogs: ICU MessageFormat strings by locale, then by message id. Stepclimb ships
// English only at 1.0; English is the locale every other one falls back to, so it is required.

/** ICU messages by id. */
export type Messages = Readonly<Record<string, string>>;

/** One package's messages by locale. Translations (1.x) add locales beside `en`. */
export type Catalog = { readonly en: Messages } & Readonly<Partial<Record<string, Messages>>>;

/** The messages Stepclimb UI's own components render. Each component adds its ids here. */
export const uiMessages: Catalog = {
  en: {
    "alertDialog.cancel": "Cancel",
    "announcement.byline": "Posted by {author} on {date}",
    "appShell.skipToMain": "Skip to main content",
    "bottomTabs.label": "Main",
    "breadcrumbs.label": "Breadcrumbs",
    "button.pending": "In progress",
    "calendar.next": "Next month",
    "calendar.previous": "Previous month",
    "callout.kind":
      "{kind, select, info {Note} tip {Tip} warning {Warning} danger {Danger} other {Note}}",
    "comboBox.empty": "No matches",
    "comboBox.loading": "Loading",
    "comboBox.showSuggestions": "Show suggestions",
    "commandPalette.empty": "No commands match your search.",
    "commandPalette.label": "Commands",
    "commandPalette.placeholder": "Type a command",
    "commandPalette.results": "Matching commands",
    "commandPalette.search": "Search commands",
    "copyButton.copied": "Copied",
    "copyButton.failed": "Could not copy. Select the text and copy it yourself.",
    "copyButton.label": "Copy",
    "courseHeader.progress": "{Course} progress",
    "courseHeader.status":
      "{status, select, draft {Draft} published {Published} archived {Archived} other {Unknown}}",
    "dataList.selectItem": "Select",
    "dataTable.loading": "Loading",
    "datePicker.openCalendar": "Open calendar",
    "dateRangePicker.separator": "–",
    "dialog.close": "Close",
    "dueBadge.due": "{days, plural, =0 {Due today} =1 {Due tomorrow} other {Due in # days}}",
    "dueBadge.overdue": "{days, plural, =0 {Overdue} one {# day overdue} other {# days overdue}}",
    "field.invalid": "Check what you entered in this field.",
    "field.valueMissing": "Fill in this field.",
    "fileDrop.choose": "{multiple, select, yes {Choose files} other {Choose file}}",
    "fileDrop.dropTarget": "Drop area",
    "fileDrop.limit": "Up to {size} per file.",
    "fileDrop.prompt": "{multiple, select, yes {Drag files here} other {Drag a file here}}",
    "fileDrop.size":
      "{unit, select, GB {{value, number, ::unit/gigabyte .#}} MB {{value, number, ::unit/megabyte .#}} other {{value, number, ::unit/kilobyte .#}}}",
    "fileDrop.tooLarge": "{name} is larger than {size}. Choose a smaller file.",
    "fileDrop.tooMany": "Drop or choose one file at a time.",
    "fileDrop.uploading": "Uploading {name}",
    "fileDrop.wrongType": "{name} is not a type of file this field accepts. Choose another file.",
    "itemRow.kind":
      "{kind, select, page {Page} file {File} external_link {External link} video {Video} quiz {Quiz} activity {Activity} other {Item}}",
    "link.external": "(external site)",
    "lockReason.moduleLocked": "Unlocks when its module unlocks",
    "lockReason.prerequisites": "Unlocks after {titles}",
    "lockReason.sequential": "Unlocks after {title}",
    "lockReason.unlockAt": "Opens on {when}",
    "meter.value": "{value, number, percent}",
    "moduleRow.number": "{number, number}",
    "moduleRow.progress": "{completed, number} / {required, number}",
    "moduleRow.progressLabel": "{completed, number} of {required, number} complete",
    "nextUp.completed": "Completed {when}",
    "nextUp.continue": "Continue",
    "nextUp.continueItem": "Continue: {item}",
    "nextUp.heading": "Next up",
    "nextUp.progress": "{Course} progress",
    "numberField.decrement": "Decrease",
    "numberField.increment": "Increase",
    "orgSwitcher.label": "{name}: switch organization",
    "pagination.arrived":
      "{direction, select, next {Showing the next page} other {Showing the previous page}}",
    "pagination.count": "{count, plural, one {# result} other {# results}}",
    "pagination.label": "Pagination",
    "pagination.next": "Next",
    "pagination.previous": "Previous",
    "progressBar.value": "{value, number, percent}",
    "prose.codeBlock": "Code block",
    "progressRail.item": "{title}, {state}",
    "progressRail.overdueItem": "{title}, {due}",
    "requirementLabel.markDone": "Mark as done",
    "requirementLabel.view": "View",
    "requirementLabel.watch": "Watch {share, number, percent}",
    "searchField.clear": "Clear search",
    "select.placeholder": "Choose one",
    "sheet.close": "Close",
    "sideNav.label": "Main",
    "statusChip.state":
      "{state, select, locked {Locked} available {Available} progress {In progress} complete {Complete} overdue {Overdue} review {Needs review} other {Unknown}}",
    "table.selectAll": "Select all",
    "table.selectRow": "Select",
    "toast.close": "Dismiss notification",
    "toast.region": "Notifications",
    "tree.toggle": "{expanded, select, yes {Collapse} other {Expand}}",
  },
};

/**
 * Merges the catalogs' messages for one locale. An id defined twice is an error, so the app's
 * catalog cannot silently replace a component's message.
 */
export function mergeMessages(catalogs: readonly Catalog[], locale: string): Map<string, string> {
  const merged = new Map<string, string>();
  for (const catalog of catalogs) {
    for (const [id, message] of Object.entries(catalog[locale] ?? {})) {
      if (merged.has(id)) throw new Error(`message id "${id}" is defined in two catalogs`);
      merged.set(id, message);
    }
  }
  return merged;
}
