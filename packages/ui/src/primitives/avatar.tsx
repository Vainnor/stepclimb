// SPDX-License-Identifier: AGPL-3.0-or-later
// Avatar: a person's picture in a pill, or their initials from the display name when there is no
// picture or it fails to load (docs/specs/m0/13-base-ui-components.md M0 inventory). Built from
// plain elements: React Aria has no avatar.
import { useState } from "react";
import { tv, type VariantProps } from "../lib/tv";

export const avatar = tv({
  slots: {
    root: [
      "relative inline-flex shrink-0 select-none items-center justify-center overflow-hidden",
      "rounded-pill border border-transparent bg-brand-soft align-middle font-ui font-medium text-brand-fg",
    ],
    image: "size-full object-cover",
    initials: "leading-none",
  },
  variants: {
    size: {
      sm: { root: "size-6 text-xs" },
      md: { root: "size-8 text-sm" },
      lg: { root: "size-12 text-lg" },
    },
  },
  defaultVariants: { size: "md" },
});

/**
 * Up to two initials: the first letter of the first and of the last word of `name`, upper-cased in
 * `locale`. A letter is a whole grapheme, so an accented or combined letter stays whole.
 */
export function initials(name: string, locale?: string): string {
  const words = name.trim().split(/\s+/u).filter(Boolean);
  const segmenter = new Intl.Segmenter(locale, { granularity: "grapheme" });
  const first = (word: string | undefined) =>
    word === undefined
      ? ""
      : (segmenter.segment(word)[Symbol.iterator]().next().value?.segment ?? "");
  const last = words.length > 1 ? words.at(-1) : undefined;
  return (first(words[0]) + first(last)).toLocaleUpperCase(locale);
}

export interface AvatarProps extends VariantProps<typeof avatar> {
  /** The person's display name: the accessible name, and the source of the initials. */
  name: string;
  /** The picture's URL. Without one, or when it fails to load, the initials show. */
  src?: string;
  /** Layout only. */
  className?: string;
}

export function Avatar({ name, src, size, className }: AvatarProps) {
  // The URL that failed, so a new `src` gets its own attempt.
  const [failed, setFailed] = useState<string | undefined>(undefined);
  const styles = avatar({ size });
  const showImage = src !== undefined && src !== failed;
  return (
    <span data-slot="avatar" role="img" aria-label={name} className={styles.root({ className })}>
      {showImage ? (
        <img
          data-slot="avatar-image"
          src={src}
          alt=""
          className={styles.image()}
          onError={() => setFailed(src)}
        />
      ) : (
        <span data-slot="avatar-initials" aria-hidden="true" className={styles.initials()}>
          {initials(name)}
        </span>
      )}
    </span>
  );
}
