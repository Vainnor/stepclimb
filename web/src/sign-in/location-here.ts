// SPDX-License-Identifier: AGPL-3.0-or-later
// A value read from the location of the step being shown. While a navigation away from the step is
// pending, the router's location is already the next one, though the step is still drawn: its
// fragment or history state is not the step's any more. The step keeps the last value it read at
// its own path, and still follows a change made there (a new fragment pasted into the same tab).
import { type ParsedLocation, useLocation } from "@tanstack/react-router";
import { useRef } from "react";

function pathOf(location: ParsedLocation): string {
  return location.pathname.length > 1 ? location.pathname.replace(/\/$/, "") : location.pathname;
}

export function useLocationHere<T>(path: string, select: (location: ParsedLocation) => T): T {
  const here = useLocation({
    select: (location) => (pathOf(location) === path ? { value: select(location) } : undefined),
  });
  const last = useRef(here);
  if (here !== undefined) last.current = here;
  // The step renders first at its own path, so `last` has a value from then on.
  return (last.current as { value: T }).value;
}
