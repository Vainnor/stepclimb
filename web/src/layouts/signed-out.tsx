// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-out layout (docs/specs/m0/11-app-shell.md B2, B8): no navigation and no TopBar; the
// organization's name, then the view. Until the sign-in screens read the organization's name and
// logo from `/.well-known/stepclimb` (spec 11 I5), it shows the product name.
import { Text, useStepclimbStrings } from "@stepclimb/ui";
import { Outlet } from "@tanstack/react-router";
import type { ReactNode } from "react";

/** The frame around a signed-out view; the view brings its own `h1`. */
export function SignedOutFrame({ children }: { children: ReactNode }) {
  const { t } = useStepclimbStrings();
  return (
    <div
      data-slot="signed-out"
      className={[
        "flex min-h-dvh flex-col items-center bg-canvas",
        "pt-[max(2.5rem,env(safe-area-inset-top))] pb-[max(2.5rem,env(safe-area-inset-bottom))]",
        "ps-[max(1rem,env(safe-area-inset-left))] pe-[max(1rem,env(safe-area-inset-right))]",
        "rtl:ps-[max(1rem,env(safe-area-inset-right))] rtl:pe-[max(1rem,env(safe-area-inset-left))]",
      ].join(" ")}
    >
      <div className="flex w-full max-w-md flex-col gap-8">
        <Text size="lg" tone="strong">
          {t("app.name")}
        </Text>
        <main id="main" tabIndex={-1} className="flex flex-col gap-4 outline-none">
          {children}
        </main>
      </div>
    </div>
  );
}

export function SignedOutLayout() {
  return (
    <SignedOutFrame>
      <Outlet />
    </SignedOutFrame>
  );
}
