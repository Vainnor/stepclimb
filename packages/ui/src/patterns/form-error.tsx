// SPDX-License-Identifier: AGPL-3.0-or-later
// FormError: why a form's submission failed, at the top of the form, when the failure belongs to no
// one field (spec 11 B17). Body-size text in the danger color after a warning icon, so color is not
// the only sign; announced when it appears. An error code the app has no message for follows the
// message in Code, so a person can report it.
import { WarningCircleIcon } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { tv } from "../lib/tv";
import { Code } from "../primitives/code";
import { Icon } from "../theme/icon";

export const formError = tv({
  slots: {
    root: "flex items-start gap-2 font-ui text-base text-danger-fg",
    icon: "mt-[0.2em] size-[1.1em] shrink-0",
    message: "flex min-w-0 flex-col gap-1",
  },
});

export interface FormErrorProps {
  /** What happened and what to do, from `useStepclimbStrings()`. */
  children: ReactNode;
  /** The server's error code, shown when the app has no message of its own for it. */
  code?: string;
  /** Layout only. */
  className?: string;
}

export function FormError({ children, code, className }: FormErrorProps) {
  const styles = formError();
  return (
    <div data-slot="form-error" role="alert" className={styles.root({ className })}>
      <span data-slot="form-error-icon" className={styles.icon()}>
        <Icon icon={WarningCircleIcon} className="size-full" />
      </span>
      <p data-slot="form-error-message" className={styles.message()}>
        <span>{children}</span>
        {code === undefined ? null : <Code data-slot="form-error-code">{code}</Code>}
      </p>
    </div>
  );
}
