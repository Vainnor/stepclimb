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
      "Your organization has turned off passkeys, so they cannot be shown or changed here. Ask an administrator how to sign in.",
    "account.passkeys.problem.notAllowed":
      "You sign in through an identity provider that does not let its users add passkeys, so you cannot add one. An administrator can change this in the provider's settings.",
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
    "problem.orgNone.description":
      "An administrator has to create the organization on the server. Try again once that is done.",
    "problem.orgNone.title": "This server has no organization yet",
    "signIn.continue": "Continue",
    "signIn.email.label": "Email",
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
    "signIn.problem.failed": "Something went wrong while signing you in. Try again.",
    "signIn.problem.flowExpired": "The passkey request expired. Try again.",
    "signIn.problem.flowInvalid":
      "This sign-in was already finished or replaced by one in another tab. Try again.",
    "signIn.problem.methodDisabled":
      "Your organization has turned off passkey sign-in. Ask an administrator how to sign in.",
    "signIn.problem.orgNone":
      "This server has no organization yet. An administrator has to create it first.",
    "signIn.problem.passkeyInvalid":
      "Stepclimb did not accept that passkey. Try again with a passkey saved for this email.",
    "signIn.problem.passkeysNotAllowed":
      "Your organization does not let you sign in with a passkey. Sign in with your identity provider instead.",
    "signIn.problem.rateLimited":
      "Too many sign-in attempts from your network. Wait a minute, then try again.",
    "signIn.problem.rateLimitedFor":
      "Too many sign-in attempts from your network. Try again in {seconds, plural, one {# second} other {# seconds}}.",
    "signIn.problem.suspended": "Your account is suspended. Ask an administrator to restore it.",
    "signIn.problem.unknown":
      "Something went wrong while signing you in. Try again. If it keeps happening, give an administrator this code:",
    "signIn.problem.unreachable":
      "Stepclimb could not reach the server. Check your connection, then try again.",
    "signIn.title": "Sign in",
    "signOut.failed": "Sign-out failed",
    "signOut.failedHint": "You are still signed in. Try again in a moment.",
    "userMenu.density": "Density",
    "userMenu.label": "{name}, account menu",
    "userMenu.saving": "{setting}: saving {choice}",
    "userMenu.signOut": "Sign out",
    "userMenu.theme": "Theme",
  },
};

/** Every catalog the app's StringsProvider reads: Stepclimb UI's, then the app's. */
export const catalogs: readonly Catalog[] = [uiMessages, webMessages];
