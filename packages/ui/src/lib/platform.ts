// SPDX-License-Identifier: AGPL-3.0-or-later
// Whether the page runs on an Apple platform (macOS, iOS, iPadOS), where the command key takes the
// place other platforms give Ctrl in shortcuts. The same test as React Aria's `isMac()` and
// `isIOS()`, which react-aria-components does not export: the platform name starts with Mac,
// iPhone or iPad (iPadOS reports itself as a Mac with a touch screen, which "Mac" covers).

interface NavigatorWithUAData extends Navigator {
  userAgentData?: { platform: string };
}

export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  const nav = navigator as NavigatorWithUAData;
  const platform = nav.userAgentData?.platform ?? nav.platform;
  return /^(Mac|iPhone|iPad)/i.test(platform);
}
