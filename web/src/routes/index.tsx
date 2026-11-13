// SPDX-License-Identifier: AGPL-3.0-or-later
// The skeleton route: reads /healthz through the generated API client.
import { Button } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api";
import { messages } from "../messages";

export function HealthRoute() {
  const health = useQuery({
    queryKey: ["healthz"],
    queryFn: async () => {
      const { data, error } = await api.GET("/healthz");
      if (error || !data) throw new Error("healthz failed");
      return data;
    },
    retry: false,
  });

  let status: string = messages.checking;
  if (health.isError) status = messages.unreachable;
  else if (health.data)
    status = health.data.status === "ok" ? messages.statusOk : messages.statusUnavailable;

  return (
    <main className="mx-auto flex max-w-xl flex-col gap-4 p-6">
      <h1 className="text-3xl font-bold text-fg-strong">{messages.appName}</h1>
      <p className="text-base">
        {messages.serverStatus}: <output data-testid="health-status">{status}</output>
      </p>
      <div>
        <Button onPress={() => health.refetch()}>{messages.recheck}</Button>
      </div>
    </main>
  );
}
