// SPDX-License-Identifier: AGPL-3.0-or-later
// Home, the one signed-in destination in M0 (docs/specs/m0/11-app-shell.md Q2). The dashboard
// with next-up items and deadlines comes with courses in M1; until then it says why it is empty.
import { HouseIcon } from "@phosphor-icons/react";
import { EmptyState, useStepclimbStrings } from "@stepclimb/ui";

export function HomeView() {
  const { t } = useStepclimbStrings();
  return (
    <div className="p-4 md:p-6">
      <EmptyState icon={HouseIcon}>{t("home.empty")}</EmptyState>
    </div>
  );
}
