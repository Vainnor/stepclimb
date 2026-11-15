// SPDX-License-Identifier: AGPL-3.0-or-later
// Code: a short machine value inside running text, such as an error code or an ID, in the mono face
// (plan UI tab -> Pitfalls: monospace only for times, codes, IDs, scores and phraseology). It reads
// left to right in every direction and may break anywhere, so a long ID never widens its column.
import type { HTMLAttributes } from "react";
import { tv } from "../lib/tv";

export const code = tv({ base: "font-mono text-fg-strong wrap-anywhere" });

export interface CodeProps extends Omit<HTMLAttributes<HTMLElement>, "className" | "dir"> {
  /** Layout only. */
  className?: string;
}

/** A `code` element. For a block of code, use Prose. */
export function Code({ className, ...props }: CodeProps) {
  return <code data-slot="code" dir="ltr" {...props} className={code({ className })} />;
}
