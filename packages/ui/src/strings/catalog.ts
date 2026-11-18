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
    "moduleBoard.drag": "Drag {name}",
    "moduleBoard.edit": "Edit {name}",
    "moduleBoard.editTitle": "Edit {title}",
    "moduleBoard.failed": "The move was not saved. Try again.",
    "moduleBoard.header": "Header",
    "moduleBoard.inModule": "in {number, number} {title}",
    "moduleBoard.itemName": "{title}, item {position, number} of module {number, number}",
    "moduleBoard.label": "Modules and items",
    "moduleBoard.moduleName": "{number, number} {title}",
    "moduleBoard.move": "Move {name}",
    "moduleBoard.moveDown": "Move down",
    "moduleBoard.moveToModule": "Move to module",
    "moduleBoard.moveUp": "Move up",
    "moduleBoard.movedItem":
      "Moved {title} to position {position, number} of {count, number} in {module}.",
    "moduleBoard.movedModule": "Moved {title} to position {position, number} of {count, number}.",
    "moduleBoard.noItems": "No items yet",
    "moduleBoard.published": "{published, select, yes {Published} other {Unpublished}}",
    "moduleBoard.saving": "Saving the new order.",
    "moduleBoard.sequential": "Items in order",
    "moduleBoard.stale":
      "The order was changed elsewhere while you were moving. The latest order is shown; make your move again.",
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
    "videoUploader.busy":
      "{name} is still uploading. Wait for it to finish, or cancel it, before choosing another video.",
    "videoUploader.bytes": "{share, number, percent} · {sent} of {size}",
    "videoUploader.cancel": "Cancel upload",
    "videoUploader.cancelConfirm.body": "The {size} sent so far will be deleted.",
    "videoUploader.cancelConfirm.confirm": "Delete upload",
    "videoUploader.cancelConfirm.keep": "Keep uploading",
    "videoUploader.cancelConfirm.title": "Cancel this upload?",
    "videoUploader.discardConfirm.keep": "Keep it",
    "videoUploader.discardConfirm.title": "Discard the upload of {name}?",
    "videoUploader.dropTarget": "Drop a video here",
    "videoUploader.pause": "Pause",
    "videoUploader.pending.description":
      "To continue an upload, choose the same file again. An unfinished upload is removed after a day without progress.",
    "videoUploader.pending.discard": "Discard",
    "videoUploader.pending.discardLabel": "Discard the upload of {name}",
    "videoUploader.pending.heading": "Unfinished uploads",
    "videoUploader.pending.item": "{name}: {share, number, percent} uploaded",
    "videoUploader.phase":
      "{phase, select, starting {Starting the upload of {name}} uploading {Uploading {name}} paused {{name}: paused} interrupted {{name}: upload stopped} cancelling {Canceling the upload of {name}} processing {{name}: processing} ready {{name} is ready} failed {{name} could not be added} cancelled {Upload of {name} canceled} other {{name}}}",
    "videoUploader.pollFailing":
      "The server could not be asked whether the video is ready. Checking again in 10 seconds.",
    "videoUploader.problem.alreadyComplete":
      "{name} has finished uploading and is being processed, so its upload can no longer be canceled.",
    "videoUploader.problem.archived": "This {course} is archived, so its videos cannot be changed.",
    "videoUploader.problem.bodyTimeout":
      "The connection stalled. Select Resume to continue from where it stopped.",
    "videoUploader.problem.courseGone":
      "This {course} could not be found. It may have been deleted, or you no longer have access to it.",
    "videoUploader.problem.csrf":
      "The server refused the request because it did not seem to come from this site. Reload the page and try again.",
    "videoUploader.problem.forbidden":
      "You do not have permission to add or change videos in this {course}.",
    "videoUploader.problem.outOfStep":
      "The upload of {name} got out of step with the server. Choose the file again to continue.",
    "videoUploader.problem.planLimit":
      "{name} could not be prepared: your organization has used the video storage or processing time its plan allows. Ask an administrator about the plan.",
    "videoUploader.problem.probeFailed":
      "{name} could not be read as a video. It may be damaged: export it again and upload the new file.",
    "videoUploader.problem.protocol":
      "This page sent the upload in a form the server does not accept. Reload the page and choose the file again; if it happens again, report this code:",
    "videoUploader.problem.serverFailed":
      "Something went wrong on the server. Try again in a moment.",
    "videoUploader.problem.signedOut":
      "You were signed out. Sign in again, then choose {name} again to continue its upload.",
    "videoUploader.problem.storageFull":
      "There is no room for {name}: your organization's video storage is full. Delete videos you no longer need, or ask an administrator about the plan.",
    "videoUploader.problem.storageNotConfigured":
      "Video storage is not set up on this server. Ask whoever runs Stepclimb for your organization to set it up.",
    "videoUploader.problem.titleInvalid":
      "The video's title comes from its file name, and {name} cannot be used as one. Rename the file and choose it again.",
    "videoUploader.problem.tooLarge":
      "{name} is larger than this server accepts for a video. Choose a smaller file, or ask whoever runs Stepclimb to raise the limit.",
    "videoUploader.problem.tooLong":
      "{name} is longer than this server accepts. Split it into shorter videos, or ask whoever runs Stepclimb to raise the limit.",
    "videoUploader.problem.transcodeFailed":
      "{name} could not be prepared for playback. Upload it again; if that fails too, export it in another format first.",
    "videoUploader.problem.unknown": "The server refused the upload.",
    "videoUploader.problem.unreachable": "The connection to the server was lost.",
    "videoUploader.problem.uploadGone":
      "The upload of {name} no longer exists: it was ended, or removed after a day without progress. Upload the video again.",
    "videoUploader.problem.uploadsBusy":
      "The server is receiving as many uploads as it can take. Select Resume in a moment.",
    "videoUploader.problem.videoFailed": "The video could not be prepared for playback.",
    "videoUploader.problem.videoGone": "{name} was deleted while it was being processed.",
    "videoUploader.problem.notAVideo":
      "{name} is not a video Stepclimb can read. Upload an MP4, MOV, WebM or MKV file.",
    "videoUploader.processingNote":
      "The server is preparing the video for playback, which can take several minutes. You can leave this page: the list of videos shows when it is ready.",
    "videoUploader.progress": "{name}: {share, number, percent} uploaded",
    "videoUploader.progressLabel": "Upload of {name}",
    "videoUploader.restarted":
      "The earlier upload of {name} had ended, so this one started from the beginning.",
    "videoUploader.resume": "Resume",
    "videoUploader.resumeNote":
      "The upload goes on by itself when the connection is back, or select Resume.",
    "videoUploader.resumed": "Continuing the earlier upload of {name} from where it stopped.",
    "videoUploader.types": "An MP4, MOV, WebM or MKV video.",
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
