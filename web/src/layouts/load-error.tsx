// SPDX-License-Identifier: AGPL-3.0-or-later
// A load that failed, with "Try again" (docs/specs/m0/11-app-shell.md B28, B29): the problem's
// `title` when the server answered with one, or a sentence saying the server could not be reached
// when it did not answer at all. It never signs the user out.
import { Button, ErrorState, type HeadingLevel, useStepclimbStrings } from "@stepclimb/ui";
import { ApiError } from "../api";

export interface LoadErrorProps {
  error: unknown;
  level: HeadingLevel;
  onRetry: () => void;
}

export function LoadError({ error, level, onRetry }: LoadErrorProps) {
  const { t } = useStepclimbStrings();
  const answered = error instanceof ApiError;
  const title = answered
    ? (error.problem?.title ?? t("loadError.title"))
    : t("loadError.unreachableTitle");
  return (
    <ErrorState
      title={title}
      level={level}
      action={<Button onPress={onRetry}>{t("loadError.retry")}</Button>}
    >
      {answered ? t("loadError.failed") : t("loadError.unreachable")}
    </ErrorState>
  );
}
