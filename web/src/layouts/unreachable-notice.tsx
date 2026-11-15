// SPDX-License-Identifier: AGPL-3.0-or-later
// The signed-in shell's offline state (docs/specs/m0/11-app-shell.md B29). When a query whose data
// is on screen tried again in the background (the tab came back into view, the network came back)
// and the server did not answer at all, the shell says so above the view, keeps drawing what it
// has, and offers "Try again". It never signs the user out. The notice goes once every such query
// has an answer again: after "Try again", or by itself when the browser's network returns and the
// queries refetch.
import { WifiSlashIcon } from "@phosphor-icons/react";
import { Button, Icon, Text, useStepclimbStrings } from "@stepclimb/ui";
import { type Query, useIsFetching, useQueryClient } from "@tanstack/react-query";
import { useCallback, useSyncExternalStore } from "react";
import { isUnreachable } from "../api";

/** A query whose data is drawn while its latest fetch got no answer. */
export function isStaleAndUnreachable(query: Query): boolean {
  return query.state.data !== undefined && isUnreachable(query.state.error);
}

/** Whether any query is stale and unreachable, "Try again" for them, and whether it is under way. */
export function useUnreachable() {
  const queryClient = useQueryClient();
  const cache = queryClient.getQueryCache();
  const unreachable = useSyncExternalStore(
    useCallback((onChange) => cache.subscribe(onChange), [cache]),
    () => cache.findAll({ predicate: isStaleAndUnreachable }).length > 0,
  );
  const retrying = useIsFetching({ predicate: isStaleAndUnreachable }) > 0;
  const retry = () => {
    void queryClient.refetchQueries({ predicate: isStaleAndUnreachable });
  };
  return { unreachable, retry, retrying };
}

export function UnreachableNotice() {
  const { t } = useStepclimbStrings();
  const { unreachable, retry, retrying } = useUnreachable();
  // The live region stays in the page, so a screen reader announces the notice when it appears.
  return (
    <div data-slot="unreachable-notice" role="status">
      {unreachable ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 pt-4 md:px-6 md:pt-6">
          <Text className="flex min-w-0 flex-1 items-start gap-2">
            <Icon icon={WifiSlashIcon} className="mt-[0.2em] shrink-0" />
            <span>{t("offline.notice")}</span>
          </Text>
          <Button onPress={retry} isPending={retrying}>
            {t("loadError.retry")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
