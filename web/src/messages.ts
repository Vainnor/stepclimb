// SPDX-License-Identifier: AGPL-3.0-or-later
// The web app's English messages (ICU MessageFormat), looked up through `useStepclimbStrings()`.
import { type Catalog, uiMessages } from "@stepclimb/ui";

export const webMessages: Catalog = {
  en: {
    "account.cancel": "Cancel",
    "account.passkeys.add": "Add a passkey",
    "account.passkeys.add.description":
      "Your browser will ask you to create the passkey with your device's fingerprint, face or screen lock, or with a security key.",
    "account.passkeys.add.nameDescription":
      "Optional. Name it after the device that holds it, so you can tell your passkeys apart.",
    "account.passkeys.add.submit": "Create the passkey",
    "account.passkeys.add.title": "Add a passkey",
    "account.passkeys.added": "{name} added",
    "account.passkeys.delete": "Delete",
    "account.passkeys.delete.confirm": "Delete passkey",
    "account.passkeys.delete.description":
      "You can no longer sign in with this passkey. This cannot be undone.",
    "account.passkeys.delete.title": "Delete {name}?",
    "account.passkeys.deleteLabel": "Delete {name}",
    "account.passkeys.deleted": "{name} deleted",
    "account.passkeys.description":
      "Passkeys let you sign in with your device's fingerprint, face or screen lock instead of a password.",
    "account.passkeys.empty": "You have no passkeys yet.",
    "account.passkeys.exists":
      "This device already holds a passkey for your account. Use another device or a security key.",
    "account.passkeys.name.empty": "Enter a name.",
    "account.passkeys.name.invalid": "This name cannot be used. Try another one.",
    "account.passkeys.name.label": "Name",
    "account.passkeys.name.tooLong": "Use at most 200 characters.",
    "account.passkeys.problem.flowExpired": "The passkey registration expired. Try again.",
    "account.passkeys.problem.flowInvalid":
      "This passkey registration was already finished or replaced by one in another tab. Try again.",
    "account.passkeys.problem.lastMethod":
      "This passkey is your last way to sign in, so it cannot be deleted. Add another passkey first, then delete this one.",
    "account.passkeys.problem.methodDisabled":
      "Your organization has turned off passkeys, so they cannot be shown or changed here. Ask an administrator if you need them.",
    "account.passkeys.problem.notAllowed":
      "Your organization's sign-in settings for this provider do not allow passkeys, so you cannot add one. An administrator of your organization can allow them in Stepclimb.",
    "account.passkeys.problem.notFound":
      "This passkey no longer exists. It may have been deleted in another tab.",
    "account.passkeys.problem.passkeyInvalid":
      "Stepclimb could not add that passkey. Try again, or use another device.",
    "account.passkeys.rename": "Rename",
    "account.passkeys.rename.submit": "Save name",
    "account.passkeys.rename.title": "Rename passkey",
    "account.passkeys.renameLabel": "Rename {name}",
    "account.passkeys.title": "Passkeys",
    "account.passkeys.unused": "Added {created, date, medium} · Not used to sign in yet",
    "account.passkeys.used": "Added {created, date, medium} · Last used {lastUsed, date, medium}",
    "account.problem.failed": "Something went wrong. Try again.",
    "account.problem.unknown":
      "Something went wrong. Try again. If it keeps happening, give an administrator this code:",
    "account.problem.unreachable":
      "Stepclimb could not reach the server. Check your connection, then try again.",
    "account.title": "Account",
    "app.name": "Stepclimb",
    "appearance.density.comfortable": "Comfortable",
    "appearance.density.compact": "Compact",
    "appearance.failed": "Your choice was not saved",
    "appearance.failedHint": "The page keeps its look. Try again in a moment.",
    "appearance.theme.dark": "Dark",
    "appearance.theme.highContrast": "High contrast",
    "appearance.theme.light": "Light",
    "appearance.theme.system": "Match my device",
    "authoring.added": "{title} added",
    "authoring.archived": "This {course} is archived, so its content cannot be changed.",
    "authoring.cancel": "Cancel",
    "authoring.courseLine":
      "{code} · {status, select, draft {Draft} published {Published} archived {Archived} other {Unknown status}}",
    "authoring.deleted": "{title} deleted",
    "authoring.empty": "This {course} has no modules.",
    "authoring.empty.edit": "This {course} has no modules yet. Add the first one above.",
    "authoring.field.completion": "Complete when the learner meets",
    "authoring.field.completion.all": "Every requirement in the module",
    "authoring.field.completion.one": "Any one requirement in the module",
    "authoring.field.kind": "Kind",
    "authoring.field.minPercent": "Share of the video to watch (%)",
    "authoring.field.module": "Module",
    "authoring.field.prerequisites": "Unlocks after",
    "authoring.field.prerequisites.description":
      "Learners must complete the modules you check before this one opens.",
    "authoring.field.published": "Published, so learners see it",
    "authoring.field.requirement": "To complete it, a learner must",
    "authoring.field.requirement.markDone": "Mark it as done",
    "authoring.field.requirement.none": "Nothing: it has no requirement",
    "authoring.field.requirement.view": "Open it",
    "authoring.field.requirement.watch": "Watch part of the video",
    "authoring.field.sequential": "Items in order",
    "authoring.field.sequential.description":
      "Learners open each item only after completing the requirements before it.",
    "authoring.field.title": "Title",
    "authoring.field.unlockAt": "Opens on",
    "authoring.field.unlockAt.clear": "Remove the date",
    "authoring.field.unlockAt.description":
      "Optional. Until this date and time, the module stays locked to learners.",
    "authoring.field.url": "Address",
    "authoring.field.url.description":
      "A full web address starting with https:// or http://. It opens in a new tab.",
    "authoring.files": "Open the files of this {course}",
    "authoring.forbidden.description":
      "Only staff who can see everything in this {course} can edit its content.",
    "authoring.forbidden.title": "You cannot edit this {course}",
    "authoring.item.add": "Add item",
    "authoring.item.add.submit": "Add item",
    "authoring.item.add.title": "Add an item",
    "authoring.item.delete": "Delete item",
    "authoring.item.delete.description":
      "The item leaves its module. What it points at, such as a page or a file, stays in this {course}. This cannot be undone.",
    "authoring.item.delete.title": "Delete {title}?",
    "authoring.item.editPage": "Open the page editor",
    "authoring.item.form": "Item details",
    "authoring.kind.externalLink.description": "A link to a page on another site.",
    "authoring.kind.header.description": "A label that groups the items below it.",
    "authoring.kind.later":
      "Pages, files and videos cannot be added to modules yet. You can already write pages, upload files and upload videos for this {course}; adding them to a module comes in a later release.",
    "authoring.module.add": "Add module",
    "authoring.module.add.description": "New modules are unpublished and go last.",
    "authoring.module.add.submit": "Add module",
    "authoring.module.add.title": "Add a module",
    "authoring.module.delete": "Delete module",
    "authoring.module.delete.description":
      "Only a module without items can be deleted. Modules that wait for it stop waiting. This cannot be undone.",
    "authoring.module.delete.title": "Delete {title}?",
    "authoring.module.form": "Module details",
    "authoring.pages.details": "Version {number, number} · Changed {updated, date, medium}",
    "authoring.pages.empty": "This {course} has no pages.",
    "authoring.pages.empty.edit":
      "This {course} has no pages yet. Write the first one with New page.",
    "authoring.pages.heading": "Pages",
    "authoring.pages.later":
      "Pages cannot be added to modules yet; that comes in a later release. Until then, learners do not see them.",
    "authoring.pages.more": "Show more pages",
    "authoring.pages.new": "New page",
    "authoring.pages.new.description":
      "The page opens in the editor. Its address is made from the title.",
    "authoring.pages.new.submit": "Create and edit",
    "authoring.pages.new.title": "New page",
    "authoring.panel.modules": "Modules",
    "authoring.panel.pages": "Pages",
    "authoring.panel.videos": "Videos",
    "authoring.panels": "Content",
    "authoring.problem.courseGone": "This {course} no longer exists.",
    "authoring.problem.dateOutOfRange": "Choose a date between 1970 and 2199.",
    "authoring.problem.denied": "You cannot change the content of this {course}.",
    "authoring.problem.failed": "The change was not saved. Try again.",
    "authoring.problem.itemGone":
      "This item no longer exists: another author deleted it. The list now shows the {course} as it is.",
    "authoring.problem.itemHasProgress":
      "Learners have already made progress on this item, so it cannot be deleted. Unpublish it to hide it from learners.",
    "authoring.problem.itemLimit":
      "A module holds at most 200 items. Add this one to another module.",
    "authoring.problem.moduleGone":
      "A module this change names no longer exists: another author deleted it. The list now shows the {course} as it is.",
    "authoring.problem.moduleLimit": "This {course} already has 200 modules, the most it can hold.",
    "authoring.problem.moduleNotEmpty":
      "This module still holds items. Move or delete them first, then delete the module.",
    "authoring.problem.pageSlugInvalid":
      "Use a title with at least one letter or digit from A to Z or 0 to 9.",
    "authoring.problem.pageSlugTaken":
      "This {course} already has a page with an address made from this title. Choose another title.",
    "authoring.problem.prerequisiteCycle":
      "These modules would wait for each other, so none of them could open. Uncheck the module that waits for this one.",
    "authoring.problem.prerequisitesInvalid": "Check each other module at most once.",
    "authoring.problem.prerequisitesTooMany": "A module can wait for at most 20 modules.",
    "authoring.problem.requirementInvalid": "This kind of item cannot take that requirement.",
    "authoring.problem.titleEmpty": "Enter a title.",
    "authoring.problem.titleInvalid": "This title holds a character that cannot be used.",
    "authoring.problem.titleTooLong": "Use at most 200 characters.",
    "authoring.problem.unknown": "The change was not saved.",
    "authoring.problem.unreachable":
      "The server could not be reached, so the change was not saved. Check your connection and try again.",
    "authoring.problem.urlCharacter": "This address holds a character that cannot be used.",
    "authoring.problem.urlInvalid":
      "Enter a full address starting with https:// or http://, without a user name, password or spaces.",
    "authoring.problem.urlRequired": "Enter the address the link opens.",
    "authoring.readOnly": "You can see the content of this {course} but not change it.",
    "authoring.save": "Save",
    "authoring.saved": "{title} saved",
    "authoring.title": "Edit {course}",
    "course.home.empty": "This {course} has no modules you can open yet.",
    "course.module.empty": "This module has no items you can open yet.",
    "course.returnLink": "Back to {host}",
    "course.returnNav": "Return to the site that sent you here",
    "course.title": "{Course}",
    "document.title": "{view} · {organization}",
    "document.titleSignedOut": "{view} · Stepclimb",
    "enroll.as": "You will sign in as {email}.",
    "enroll.noBrowserMethod":
      "Your organization lets this link set up only a password, which this version of Stepclimb cannot do in the browser yet. Ask an administrator to turn on passkeys, then open the link again.",
    "enroll.noToken":
      "Open the whole setup link an administrator gave you, including the part after the # sign.",
    "enroll.noToken.title": "This page needs your setup link",
    "enroll.passkey.description":
      "A passkey lets you sign in with your device's fingerprint, face or screen lock instead of a password.",
    "enroll.passkey.exists":
      "This device already has a passkey for this account. Sign in with it, or create one on another device.",
    "enroll.passkey.submit": "Create a passkey",
    "enroll.problem.expired": "It can no longer be used. Ask an administrator for a new one.",
    "enroll.problem.expiredTitle": "This setup link has expired",
    "enroll.problem.invalid":
      "It was already used, replaced by a newer link, or copied incompletely. Ask an administrator for a new one.",
    "enroll.problem.invalidTitle": "This setup link does not work",
    "enroll.problem.methodDisabled":
      "Your organization has turned off passkey and password sign-in. Ask an administrator how to sign in.",
    "enroll.problem.passkeyInvalid": "Stepclimb could not register that passkey. Try again.",
    "enroll.problem.signIn": "Go to sign in",
    "enroll.problem.title": "This setup link could not be opened",
    "enroll.title": "Set up your account",
    "files.actions": "Actions for {name}",
    "files.archived": "This {course} is archived, so its files cannot be changed.",
    "files.badFolder.description": "Check the address, or go to the top folder of this {course}.",
    "files.badFolder.title": "This folder path is not valid",
    "files.badFolder.top": "Go to the top folder",
    "files.cancel": "Cancel",
    "files.column.actions": "Actions",
    "files.column.name": "Name",
    "files.column.size": "Size",
    "files.delete": "Delete",
    "files.delete.confirm": "Delete file",
    "files.delete.description": "The file is removed from this folder. This cannot be undone.",
    "files.delete.failed": "{name} was not deleted",
    "files.delete.title": "Delete {name}?",
    "files.deleted": "{name} deleted",
    "files.details": "{type} · Changed {updated, date, medium}",
    "files.empty.course": "This {course} has no files.",
    "files.empty.courseEdit": "This {course} has no files yet. Upload the first one above.",
    "files.empty.folder": "This folder is empty.",
    "files.folderRenamed": "Folder renamed to {name}",
    "files.folders": "Folders",
    "files.forbidden.description":
      "Only staff who can see everything in this {course} can open its files. Learners reach files through the items of its modules.",
    "files.forbidden.title": "You cannot open these files",
    "files.list": "Files in {folder}",
    "files.move": "Move",
    "files.move.description":
      "A folder path such as /handouts/week-1, or / for the top folder. A folder that does not exist yet is made when the file moves into it.",
    "files.move.label": "Folder",
    "files.move.submit": "Move file",
    "files.move.title": "Move {name}",
    "files.moved": "{name} moved to {folder}",
    "files.notFound.description":
      "It may have been deleted, or you may not have access to it. Check the address.",
    "files.notFound.title": "This {course} could not be found",
    "files.problem.bodyTimeout":
      "The upload of {name} stopped sending. Check your connection, then try again.",
    "files.problem.courseGone":
      "This {course} no longer exists, or you no longer have access to it.",
    "files.problem.failed": "Something went wrong. Try again.",
    "files.problem.fileGone":
      "This file no longer exists. It may have been deleted in another tab. The list has been refreshed.",
    "files.problem.folderGone":
      "This folder no longer exists. Its files may have been moved in another tab. The list has been refreshed.",
    "files.problem.folderInvalid":
      "That folder path cannot be used. Separate folder names with /, and leave out \\, . and .. as names.",
    "files.problem.folderNameTaken":
      "Renaming the folder to {name} would put two files with the same name in one folder. Choose another name.",
    "files.problem.folderRoot": "Choose a name for the folder.",
    "files.problem.folderTooDeep": "Folders can be at most 8 levels deep.",
    "files.problem.folderTooLong": "Each folder name can be at most 100 characters.",
    "files.problem.forbidden":
      "You do not have permission to change the files of this {course}. Ask an administrator if you need to.",
    "files.problem.inUse":
      "{name} is used by an item or a page of this {course}, so it cannot be deleted. Remove it from there first.",
    "files.problem.nameInvalid":
      "That name cannot be used. Leave out / and \\, and do not use . or .. as a name.",
    "files.problem.nameTaken":
      "This folder already has a file named {name}. Rename one of them, then try again.",
    "files.problem.nameTooLong": "Use at most 255 characters.",
    "files.problem.storageFull":
      "Your organization has used all the file storage its plan allows, so {name} was not uploaded. Delete files that are no longer needed, or ask an administrator.",
    "files.problem.storageNotConfigured":
      "File storage is not set up on this server, so files cannot be uploaded. An administrator has to set STEPCLIMB_STORAGE_URL.",
    "files.problem.tooLarge":
      "{name} is larger than this server accepts. Ask an administrator for the size limit.",
    "files.problem.unknown":
      "Something went wrong. Try again. If it keeps happening, give an administrator this code:",
    "files.problem.unknownWithCode":
      "Something went wrong. Try again. If it keeps happening, give an administrator this code: {code}",
    "files.problem.unreachable":
      "Stepclimb could not reach the server. Check your connection, then try again.",
    "files.problem.uploadsBusy": "The server is busy with other uploads. Try again in a moment.",
    "files.rename": "Rename",
    "files.rename.label": "Name",
    "files.rename.submit": "Save name",
    "files.rename.title": "Rename {name}",
    "files.renameFolder.title": "Rename folder {name}",
    "files.renamed": "Renamed to {name}",
    "files.size.bytes": "{count, plural, one {# byte} other {# bytes}}",
    "files.title": "Files",
    "files.topFolder": "Top folder",
    "files.tree.failed": "Some folders could not be loaded.",
    "files.type.audio": "Audio",
    "files.type.audioVideo": "Audio or video",
    "files.type.excel": "Excel spreadsheet",
    "files.type.extension": "{extension} file",
    "files.type.image": "Image",
    "files.type.mime": "MIME type {mime}",
    "files.type.odfPresentation": "OpenDocument presentation",
    "files.type.odfSpreadsheet": "OpenDocument spreadsheet",
    "files.type.odfText": "OpenDocument text",
    "files.type.other": "File",
    "files.type.pdf": "PDF",
    "files.type.powerPoint": "PowerPoint presentation",
    "files.type.text": "Plain text",
    "files.type.video": "Video",
    "files.type.word": "Word document",
    "files.upload.busy": "{name} is still uploading. Choose more files once it has finished.",
    "files.upload.description": "Files go into {folder}.",
    "files.upload.done": "{name} uploaded",
    "files.upload.notSent":
      "{count, plural, one {The file chosen after it was not uploaded.} other {The # files chosen after it were not uploaded.}}",
    "files.upload.uploading": "Uploading {name}",
    "home.empty": "You have no {courses} yet.",
    "home.title": "Home",
    "loadError.documentTitle": "Could not load",
    "loadError.failed": "Stepclimb could not load this page. Try again in a moment.",
    "loadError.retry": "Try again",
    "loadError.title": "Could not load",
    "loadError.unreachable": "Check your connection, then try again.",
    "loadError.unreachableTitle": "The server could not be reached",
    "nav.account": "Account",
    "nav.home": "Home",
    "notFound.description": "Check the address, or go to the home page.",
    "notFound.heading": "There is no page at {path}",
    "notFound.home": "Go to the home page",
    "notFound.title": "Page not found",
    "offline.notice":
      "The server could not be reached, so this page may be out of date. Check your connection, then try again.",
    "pageEdit.conflict.label": "Newer version saved",
    "pageEdit.conflict.message":
      "Someone saved version {number} of this page while you were editing. Your changes are still here and in this browser.",
    "pageEdit.conflict.overwrite": "Save my changes as the newest version",
    "pageEdit.conflict.reload": "Load version {number}",
    "pageEdit.conflict.reloadConfirm": "Discard my changes",
    "pageEdit.conflict.reloadDescription":
      "Your unsaved changes will be lost. Version {number} stays in the page's history either way.",
    "pageEdit.conflict.reloadTitle": "Load version {number} and discard your changes?",
    "pageEdit.draft.discard": "Discard",
    "pageEdit.draft.found":
      "You have unsaved changes to this page from {when, date, medium} at {when, time, short}.",
    "pageEdit.draft.label": "Unsaved changes",
    "pageEdit.draft.restore": "Restore my changes",
    "pageEdit.loaded": "Version {number} loaded",
    "pageEdit.problem.archived": "This {course} is archived, so its pages cannot be changed.",
    "pageEdit.problem.bodyInvalid":
      "The outlined block holds something a page cannot hold. Change or delete it, then save again.",
    "pageEdit.problem.denied": "You do not have permission to edit this page.",
    "pageEdit.problem.failed":
      "The page could not be saved. Try again; your changes are kept in this browser.",
    "pageEdit.problem.fileNotFound":
      "An image on this page is not a file of this {course}. Delete it or insert it again, then save.",
    "pageEdit.problem.imageType":
      "The outlined image is not a PNG, JPEG, GIF or WebP file. Replace it, then save again.",
    "pageEdit.problem.pageGone":
      "This page no longer exists. Copy any text you want to keep before you leave.",
    "pageEdit.problem.tooLarge":
      "This page is too large to save: a page holds at most 1 MiB of content and 20,000 parts. Move some of it to another page.",
    "pageEdit.problem.unreachable":
      "Stepclimb could not reach the server. Your changes are kept in this browser; save again when you are back online.",
    "pageEdit.save": "Save",
    "pageEdit.saved": "Saved as version {number}",
    "pageEdit.state.saved": "Version {number}, all changes saved",
    "pageEdit.state.saving": "Saving",
    "pageEdit.state.unsaved": "Unsaved changes",
    "pageEdit.title": "Edit page",
    "pageEdit.upload.failed": "The image could not be uploaded. Try again.",
    "pageEdit.upload.noStorage":
      "File uploads are not set up on this server. Ask an administrator to configure storage.",
    "pageEdit.upload.storageFull":
      "Your organization has used all its file storage. Delete files you no longer need, or ask an administrator.",
    "pageEdit.upload.tooLarge":
      "This image is larger than this server accepts. Choose a smaller file.",
    "pageEdit.upload.unreachable":
      "Stepclimb could not reach the server. Check your connection, then try again.",
    "problem.orgNone.description":
      "An administrator has to create the organization on the server. Try again once that is done.",
    "problem.orgNone.title": "This server has no organization yet",
    "signIn.continue": "Continue",
    "signIn.email.label": "Email",
    "signIn.empty":
      "This organization has not turned on a way to sign in from the browser yet. Ask an administrator how to sign in.",
    "signIn.passkey.as": "Signing in as {email}.",
    "signIn.passkey.cancelled":
      "The passkey request was cancelled or timed out. Try again when you are ready.",
    "signIn.passkey.failed":
      "This browser could not use a passkey. Try again, or use another browser.",
    "signIn.passkey.insecure":
      "Passkeys work only over a secure connection. Open this page at its https:// address, then try again.",
    "signIn.passkey.otherEmail": "Use a different email",
    "signIn.passkey.submit": "Sign in with a passkey",
    "signIn.passkey.title": "Use your passkey",
    "signIn.passkey.unsupported":
      "This browser cannot use passkeys. Update it, or use a current version of Chrome, Edge, Firefox or Safari.",
    "signIn.passkey.usePassword": "Sign in with a password instead",
    "signIn.password.as": "Signing in as {email}.",
    "signIn.password.label": "Password",
    "signIn.password.submit": "Sign in",
    "signIn.password.title": "Enter your password",
    "signIn.password.usePasskey": "Sign in with a passkey instead",
    "signIn.problem.admissionRefused":
      "Your organization's sign-in rules do not let this account in. Ask an administrator.",
    "signIn.problem.credentialsInvalid":
      "That email and password do not match. Check both, then try again.",
    "signIn.problem.failed": "Something went wrong while signing you in. Try again.",
    "signIn.problem.flowExpired": "The passkey request expired. Try again.",
    "signIn.problem.flowInvalid":
      "This sign-in was already finished or replaced by one in another tab. Try again.",
    "signIn.problem.idTokenInvalid":
      "Stepclimb could not verify the answer from the sign-in service, so you are not signed in. Try again. If it keeps happening, tell an administrator.",
    "signIn.problem.limitReached":
      "This organization has no room for another user on its plan, so your account could not be created. Ask an administrator.",
    "signIn.problem.methodDisabled":
      "Your organization has turned off passkey sign-in. Ask an administrator how to sign in.",
    "signIn.problem.notProvisioned":
      "This organization lets in only people it already has an account for, and it has none for you. Ask an administrator to add you.",
    "signIn.problem.orgNone":
      "This server has no organization yet. An administrator has to create it first.",
    "signIn.problem.passkeyInvalid":
      "Stepclimb did not accept that passkey. Try again with a passkey saved for this email.",
    "signIn.problem.passkeysNotAllowed":
      "Your organization does not let you sign in with a passkey. Sign in with your identity provider instead.",
    "signIn.problem.passwordDisabled":
      "Your organization has turned off password sign-in. Ask an administrator how to sign in.",
    "signIn.problem.passwordRateLimited":
      "Too many sign-in attempts. Wait a few minutes, then try again.",
    "signIn.problem.passwordRateLimitedFor":
      "Too many sign-in attempts. Try again in {seconds, plural, one {# second} other {# seconds}}.",
    "signIn.problem.providerDenied":
      "The sign-in was declined at the sign-in service, so you are not signed in. Try again, and allow the request when the service asks.",
    "signIn.problem.providerError":
      "The sign-in service reported a problem, so you are not signed in. Try again. If it keeps happening, tell an administrator.",
    "signIn.problem.providerFlowExpired": "The sign-in took too long to finish. Start it again.",
    "signIn.problem.providerNotFound":
      "That way to sign in is not available. Choose another one below.",
    "signIn.problem.providerUnavailable":
      "Stepclimb could not start the sign-in with that service. Try again in a few minutes. If it keeps happening, tell an administrator.",
    "signIn.problem.rateLimited":
      "Too many sign-in attempts from your network. Wait a minute, then try again.",
    "signIn.problem.rateLimitedFor":
      "Too many sign-in attempts from your network. Try again in {seconds, plural, one {# second} other {# seconds}}.",
    "signIn.problem.signupClosed":
      "There is no account for you in this organization, and it does not create accounts at sign-in. Ask an administrator to add you.",
    "signIn.problem.subjectMissing":
      "The sign-in service did not say who you are, so Stepclimb cannot sign you in. Tell an administrator.",
    "signIn.problem.suspended": "Your account is suspended. Ask an administrator to restore it.",
    "signIn.problem.totpDisabled":
      "Your organization has turned off sign-in with a password and code. Ask an administrator how to sign in.",
    "signIn.problem.totpExpired":
      "The sign-in took too long to finish. Enter your password again, then a new code.",
    "signIn.problem.totpInvalid":
      "That code did not work. Enter your password again, then the newest code from your app or an unused recovery code.",
    "signIn.problem.totpRestart":
      "This sign-in was already finished or replaced by one in another tab. Enter your password again.",
    "signIn.problem.unknown":
      "Something went wrong while signing you in. Try again. If it keeps happening, give an administrator this code:",
    "signIn.problem.unreachable":
      "Stepclimb could not reach the server. Check your connection, then try again.",
    "signIn.provider.unlabeled": "Sign in with {key}",
    "signIn.title": "Sign in",
    "signIn.totp.as": "Signing in as {email}.",
    "signIn.totp.description":
      "Open your authenticator app and enter the 6-digit code it shows for this account.",
    "signIn.totp.label": "Code",
    "signIn.totp.recovery.description":
      "One of the recovery codes you saved when you turned on codes from an app. Each one works once.",
    "signIn.totp.recovery.label": "Recovery code",
    "signIn.totp.startAgain": "Start again",
    "signIn.totp.submit": "Sign in",
    "signIn.totp.title": "Enter your code",
    "signIn.totp.useApp": "Use a code from your app instead",
    "signIn.totp.useRecovery": "Use a recovery code instead",
    "signIn.totpSetup.code.description":
      "After adding the account, enter the 6-digit code your app shows for it.",
    "signIn.totpSetup.code.label": "Code",
    "signIn.totpSetup.intro":
      "Your organization asks for a code from an authenticator app each time you sign in with a password. Set one up to finish signing in as {email}.",
    "signIn.totpSetup.key": "In your authenticator app, add an account with this setup key:",
    "signIn.totpSetup.link": "Or, if your app takes a setup link, use this one:",
    "signIn.totpSetup.problem.invalid":
      "That code did not work. Check that your device's clock is right, then enter the newest code from your app.",
    "signIn.totpSetup.recovery.continue": "Continue",
    "signIn.totpSetup.recovery.intro":
      "Each of these codes signs you in once if you lose your app. Keep them somewhere safe, such as a password manager. They are not shown again.",
    "signIn.totpSetup.recovery.title": "Save your recovery codes",
    "signIn.totpSetup.submit": "Turn on and sign in",
    "signIn.totpSetup.title": "Set up a code from an app",
    "signOut.failed": "Sign-out failed",
    "signOut.failedHint": "You are still signed in. Try again in a moment.",
    "update.available": "A new version is available",
    "update.reload": "Reload",
    "userMenu.density": "Density",
    "userMenu.label": "{name}, account menu",
    "userMenu.saving": "{setting}: saving {choice}",
    "userMenu.signOut": "Sign out",
    "userMenu.theme": "Theme",
    "videos.archived": "This {course} is archived, so its videos cannot be changed.",
    "videos.details":
      "{status, select, uploading {Uploading} processing {Processing} ready {Ready} failed {Failed} other {Unknown status}} · Added {created, date, medium}",
    "videos.empty": "This {course} has no videos.",
    "videos.empty.edit": "This {course} has no videos yet. Upload the first one above.",
    "videos.forbidden.description":
      "Only staff who can see everything in this {course} can open its videos. Learners watch videos through the items of its modules.",
    "videos.forbidden.title": "You cannot open these videos",
    "videos.list": "Videos in this {course}",
    "videos.more": "Show more videos",
    "videos.title": "Videos",
  },
};

/** Every catalog the app's StringsProvider reads: Stepclimb UI's, then the app's. */
export const catalogs: readonly Catalog[] = [uiMessages, webMessages];
