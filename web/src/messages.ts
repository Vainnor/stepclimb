// SPDX-License-Identifier: AGPL-3.0-or-later
// The web app's English messages (ICU MessageFormat), looked up through `useStepclimbStrings()`.
import { type Catalog, uiMessages } from "@stepclimb/ui";

export const webMessages: Catalog = {
  en: {
    "app.name": "Stepclimb",
    "health.label": "Server status:",
    "health.checking": "Checking…",
    "health.ok": "ok",
    "health.unavailable": "unavailable",
    "health.unreachable": "The server did not answer. Is `stepclimb serve` running?",
    "health.recheck": "Check again",
  },
};

/** Every catalog the app's StringsProvider reads: Stepclimb UI's, then the app's. */
export const catalogs: readonly Catalog[] = [uiMessages, webMessages];
