// SPDX-License-Identifier: AGPL-3.0-or-later
// What the signed-in layout draws while `/me` loads (docs/specs/m0/11-app-shell.md B9): the page
// background alone for the first 300 ms, since each Skeleton waits that long before it appears,
// then the shell's outline without shimmer.
import { Skeleton } from "@stepclimb/ui";

export function ShellSkeleton() {
  return (
    <div data-slot="shell-skeleton" aria-busy="true" className="flex min-h-dvh bg-canvas">
      <div className="hidden w-16 shrink-0 flex-col gap-3 p-3 md:flex lg:w-60">
        <Skeleton shape="block" className="h-8" />
        <Skeleton shape="block" className="h-8" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-6 p-4 md:p-6">
        <Skeleton className="h-6 max-w-48" />
        <Skeleton shape="block" className="h-32" />
      </div>
    </div>
  );
}
