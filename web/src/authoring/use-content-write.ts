// SPDX-License-Identifier: AGPL-3.0-or-later
// A write of the course authoring screen: a mutation that reads the course's modules again when it
// worked, or when what it named is gone, so the board shows the server's state.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { modulesKey } from "./course-content";
import { isGone } from "./problems";

export function useContentWrite<V, R>(courseId: string, write: (value: V) => Promise<R>) {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries({ queryKey: modulesKey(courseId) });
  return useMutation({
    mutationFn: write,
    onSuccess: () => refresh(),
    onError: (error) => {
      if (isGone(error)) void refresh();
    },
  });
}
