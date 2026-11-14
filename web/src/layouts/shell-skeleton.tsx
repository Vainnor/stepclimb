// SPDX-License-Identifier: AGPL-3.0-or-later
// What the signed-in layout draws while `/me` loads (docs/specs/m0/11-app-shell.md B9): the page
// background alone for the first 300 ms, since each Skeleton waits that long before it appears,
// then the shell's shape without shimmer: the SideNav from 768 px, the TopBar, and the BottomTabs
// under 768 px, each inside the safe areas as AppShell's own parts are.
import { Skeleton } from "@stepclimb/ui";

export function ShellSkeleton() {
  return (
    <div data-slot="shell-skeleton" aria-busy="true" className="flex min-h-dvh">
      <div
        className={[
          "hidden shrink-0 flex-col gap-3 md:flex",
          "w-[calc(4rem+env(safe-area-inset-left))] lg:w-[calc(15rem+env(safe-area-inset-left))]",
          "ps-[env(safe-area-inset-left)] pt-[max(1rem,env(safe-area-inset-top))]",
          "rtl:ps-[env(safe-area-inset-right)]",
        ].join(" ")}
      >
        <div className="flex flex-col gap-3 px-3">
          <Skeleton shape="block" className="h-8" />
          <Skeleton shape="block" className="h-8" />
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          className={[
            "flex min-h-14 items-center justify-between gap-3 pt-[env(safe-area-inset-top)]",
            "ps-[max(1rem,env(safe-area-inset-left))] pe-[max(1rem,env(safe-area-inset-right))]",
            "rtl:ps-[max(1rem,env(safe-area-inset-right))] rtl:pe-[max(1rem,env(safe-area-inset-left))]",
            "md:ps-6 md:pe-6",
          ].join(" ")}
        >
          <Skeleton className="h-6 max-w-40" />
          <Skeleton shape="circle" className="size-8 shrink-0" />
        </div>
        <div className="flex flex-col gap-3 p-4 pb-[calc(3.5rem+env(safe-area-inset-bottom))] md:p-6">
          <Skeleton shape="block" className="h-32" />
        </div>
      </div>
      <div
        className={[
          "fixed inset-x-0 bottom-0 flex min-h-14 items-center justify-around md:hidden",
          "pb-[env(safe-area-inset-bottom)] ps-[env(safe-area-inset-left)] pe-[env(safe-area-inset-right)]",
          "rtl:ps-[env(safe-area-inset-right)] rtl:pe-[env(safe-area-inset-left)]",
        ].join(" ")}
      >
        <Skeleton shape="circle" className="size-6" />
      </div>
    </div>
  );
}
