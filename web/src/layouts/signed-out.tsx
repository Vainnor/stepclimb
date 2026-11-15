// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-out layout (docs/specs/m0/11-app-shell.md B2, B8, B13): no navigation and no TopBar;
// the organization's name, then the view. A signed-out browser learns the name from
// `/.well-known/stepclimb` (spec 08 B38). While the document loads the header keeps its line empty,
// so the name does not replace another word; when it cannot load, and in the frames drawn outside
// the layout (the not-found view, a failed layout), the header names the product.
import { Text, useStepclimbStrings } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { Outlet } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { discoveryQuery } from "../sign-in/discovery";

export interface SignedOutFrameProps {
  children: ReactNode;
  /**
   * The header's name: the organization's, `"pending"` while it loads, or the product's when
   * absent.
   */
  organization?: string | "pending";
}

/** The frame around a signed-out view; the view brings its own `h1`. */
export function SignedOutFrame({ children, organization }: SignedOutFrameProps) {
  const { t } = useStepclimbStrings();
  const name = organization === "pending" ? "" : (organization ?? t("app.name"));
  return (
    <div
      data-slot="signed-out"
      className={[
        "flex min-h-dvh flex-col items-center",
        "pt-[max(2.5rem,env(safe-area-inset-top))] pb-[max(2.5rem,env(safe-area-inset-bottom))]",
        "ps-[max(1rem,env(safe-area-inset-left))] pe-[max(1rem,env(safe-area-inset-right))]",
        "rtl:ps-[max(1rem,env(safe-area-inset-right))] rtl:pe-[max(1rem,env(safe-area-inset-left))]",
      ].join(" ")}
    >
      <div className="flex w-full max-w-md flex-col gap-8">
        <header>
          <Text size="lg" tone="strong" className="block min-h-[1lh] wrap-anywhere">
            {name}
          </Text>
        </header>
        <main id="main" tabIndex={-1} className="flex flex-col gap-4 outline-none">
          {children}
        </main>
      </div>
    </div>
  );
}

export function SignedOutLayout() {
  const discovery = useQuery(discoveryQuery);
  const organization = discovery.isPending ? "pending" : discovery.data?.name;
  return (
    <SignedOutFrame organization={organization}>
      <Outlet />
    </SignedOutFrame>
  );
}
