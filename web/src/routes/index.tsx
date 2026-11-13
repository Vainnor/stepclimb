// SPDX-License-Identifier: AGPL-3.0-or-later
// The skeleton route: reads /healthz through the generated API client.
import { Button, useStepclimbStrings } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api";

export function HealthRoute() {
  const { t } = useStepclimbStrings();
  const health = useQuery({
    queryKey: ["healthz"],
    queryFn: async () => {
      const { data, error } = await api.GET("/healthz");
      if (error || !data) throw new Error("healthz failed");
      return data;
    },
    retry: false,
  });

  let status = t("health.checking");
  if (health.isError) status = t("health.unreachable");
  else if (health.data)
    status = health.data.status === "ok" ? t("health.ok") : t("health.unavailable");

  return (
    <main className="mx-auto flex max-w-xl flex-col gap-4 p-6">
      <h1 className="text-3xl font-bold text-fg-strong">{t("app.name")}</h1>
      <p className="text-base">
        {t("health.label")} <output data-testid="health-status">{status}</output>
      </p>
      <div>
        <Button onPress={() => health.refetch()}>{t("health.recheck")}</Button>
      </div>
    </main>
  );
}
