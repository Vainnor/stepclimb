// SPDX-License-Identifier: AGPL-3.0-or-later
// Registers the service worker and shows "A new version is available" with "Reload" when a new
// build's worker waits (docs/specs/m1/15-installable-web-app-and-offline-reading.md B5, B6).
// Rendered once, at the root of the route tree; it draws nothing itself.
import { type ToastQueue, useStepclimbStrings } from "@stepclimb/ui";
import { useEffect, useRef } from "react";
import { type ContainerLike, registerWorker, shouldRegister } from "../service-worker";

export interface AppUpdateProps {
  toasts: ToastQueue;
  /** The page's worker container; by default the browser's, absent where it has none. */
  container?: ContainerLike;
  /** By default whether this is a production build. */
  production?: boolean;
  /** By default reloads the page. */
  reload?: () => void;
}

const browserContainer = "serviceWorker" in navigator ? navigator.serviceWorker : undefined;
const reloadPage = () => window.location.reload();

export function AppUpdate({
  toasts,
  container = browserContainer,
  production = import.meta.env.PROD,
  reload = reloadPage,
}: AppUpdateProps) {
  const { t } = useStepclimbStrings();
  // The latest strings, queue and reload, read when an update arrives, so a new render never
  // registers again or offers the same worker twice.
  const latest = useRef({ t, toasts, reload });
  latest.current = { t, toasts, reload };
  useEffect(() => {
    const where = {
      production,
      protocol: window.location.protocol,
      hostname: window.location.hostname,
      supported: container !== undefined,
    };
    if (!container || !shouldRegister(where)) return;
    const stop = new AbortController();
    const onUpdate = (apply: () => void) => {
      const { t, toasts } = latest.current;
      toasts.add({
        title: t("update.available"),
        action: { label: t("update.reload"), onAction: apply },
      });
    };
    const hooks = { onUpdate, reload: () => latest.current.reload(), signal: stop.signal };
    registerWorker(container, hooks).catch((error: unknown) => {
      // The app works without its worker: it then neither opens offline nor offers updates.
      console.error("The service worker could not be registered", error);
    });
    return () => stop.abort();
  }, [container, production]);
  return null;
}
