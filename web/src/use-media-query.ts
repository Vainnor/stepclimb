// SPDX-License-Identifier: AGPL-3.0-or-later
// Whether a media query matches, kept current as it changes: for a view that mounts a part only
// at some widths, such as the files area's folder tree from 768 px.
import { useCallback, useSyncExternalStore } from "react";

export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const list = window.matchMedia(query);
      list.addEventListener("change", onChange);
      return () => list.removeEventListener("change", onChange);
    },
    [query],
  );
  return useSyncExternalStore(
    subscribe,
    () => window.matchMedia(query).matches,
    () => false,
  );
}
