// SPDX-License-Identifier: AGPL-3.0-or-later
// Skeleton: a still placeholder for content that is loading. It appears only once the load has
// taken 300 ms, so a fast load never flashes it, and it does not shimmer
// (docs/specs/m0/13-base-ui-components.md M0 inventory; plan UI tab -> Pitfalls: animation
// everywhere). It is hidden from assistive technology: the region that is loading sets
// `aria-busy`.
import { useEffect, useState } from "react";
import { tv, type VariantProps } from "../lib/tv";

export const SKELETON_DELAY_MS = 300;

export const skeleton = tv({
  base: "block bg-sunken",
  variants: {
    shape: {
      /** A line of text: as tall as the text around it. */
      text: "h-[1em] w-full rounded-xs",
      /** A block such as an image or a card: size it with `className`. */
      block: "w-full rounded-md",
      /** An avatar: size it with `className`. */
      circle: "aspect-square rounded-pill",
    },
  },
  defaultVariants: { shape: "text" },
});

export interface SkeletonProps extends VariantProps<typeof skeleton> {
  /** Layout only: width and height. */
  className?: string;
}

export function Skeleton({ shape, className }: SkeletonProps) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), SKELETON_DELAY_MS);
    return () => clearTimeout(timer);
  }, []);
  if (!shown) return null;
  return (
    <span data-slot="skeleton" aria-hidden="true" className={skeleton({ shape, className })} />
  );
}
