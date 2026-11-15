// SPDX-License-Identifier: AGPL-3.0-or-later
// A sign-in failure at the top of the form (docs/specs/m0/11-app-shell.md B17): announced when it
// appears, drawn with a field error's look (an icon beside the text, so color is not the only
// sign), and the server's code in mono when the screen has no message of its own for it.
import { WarningCircleIcon } from "@phosphor-icons/react";
import { field, Icon } from "@stepclimb/ui";
import type { SignInProblem } from "./problems";

const styles = field();

export function FormError({ problem }: { problem: SignInProblem }) {
  return (
    <div data-slot="form-error" role="alert" className={styles.error()}>
      <Icon icon={WarningCircleIcon} className={styles.errorIcon()} />
      <p className="flex flex-col gap-1">
        <span>{problem.message}</span>
        {problem.code === undefined ? null : (
          <code data-slot="form-error-code" className="font-mono wrap-anywhere">
            {problem.code}
          </code>
        )}
      </p>
    </div>
  );
}
