// SPDX-License-Identifier: AGPL-3.0-or-later
// AppShell: the signed-in layout (docs/specs/m0/13-base-ui-components.md M0 inventory; spec 11 B3,
// B6). A skip link to `main` is its first focusable element. At 768 px and wider the SideNav sits at
// the inline start (collapsed to icons until 1024 px, which the SideNav decides); under 768 px it
// is replaced by BottomTabs, and `main` leaves room for them and the bottom safe area. The
// breakpoints are CSS, so the first paint already has the right layout. SideNav and BottomTabs
// bring the `nav` landmarks, TopBar the `header`.
import type { MouseEvent, ReactNode } from "react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";

export const appShell = tv({
  slots: {
    root: "flex min-h-dvh bg-canvas font-ui text-fg-default",
    skip: [
      "sr-only focus:not-sr-only focus:fixed focus:start-2 focus:top-2 focus:z-50",
      "focus:rounded-sm focus:bg-raised focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:text-fg-strong",
      "focus:shadow-float",
    ],
    side: "hidden shrink-0 md:block",
    column: "flex min-w-0 flex-1 flex-col",
    // Clear of the inline-end safe area (a phone in landscape); the SideNav and TopBar keep clear
    // of the others themselves.
    main: [
      "min-w-0 flex-1 outline-none",
      "pe-[env(safe-area-inset-right)] rtl:pe-[env(safe-area-inset-left)]",
    ],
    bottom: "md:hidden",
  },
  variants: {
    /** Room under the content for the BottomTabs and the bottom safe area, under 768 px. */
    hasBottomTabs: { true: { main: "pb-[calc(3.5rem+env(safe-area-inset-bottom))] md:pb-0" } },
  },
});

export interface AppShellProps {
  /** The SideNav, shown at 768 px and wider. */
  sideNav?: ReactNode;
  /** The BottomTabs, shown under 768 px. */
  bottomTabs?: ReactNode;
  /** The TopBar. */
  topBar?: ReactNode;
  /** The current view. */
  children: ReactNode;
  /** The `id` of `main`, the skip link's target. */
  mainId?: string;
  /** Layout only. */
  className?: string;
}

export function AppShell({
  sideNav,
  bottomTabs,
  topBar,
  children,
  mainId = "main",
  className,
}: AppShellProps) {
  const { t } = useStepclimbStrings();
  const styles = appShell({ hasBottomTabs: bottomTabs !== undefined });
  // Focus moves to `main` itself, so the next Tab goes into the view rather than back to the top.
  const skip = (event: MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    document.getElementById(mainId)?.focus();
  };
  return (
    <div data-slot="app-shell" className={styles.root({ className })}>
      <a data-slot="app-shell-skip" href={`#${mainId}`} onClick={skip} className={styles.skip()}>
        {t("appShell.skipToMain")}
      </a>
      {sideNav === undefined ? null : (
        <div data-slot="app-shell-side" className={styles.side()}>
          {sideNav}
        </div>
      )}
      <div data-slot="app-shell-column" className={styles.column()}>
        {topBar}
        <main data-slot="app-shell-main" id={mainId} tabIndex={-1} className={styles.main()}>
          {children}
        </main>
      </div>
      {bottomTabs === undefined ? null : (
        <div data-slot="app-shell-bottom" className={styles.bottom()}>
          {bottomTabs}
        </div>
      )}
    </div>
  );
}
