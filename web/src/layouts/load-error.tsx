// SPDX-License-Identifier: AGPL-3.0-or-later
// A load that failed, with "Try again" (docs/specs/m0/11-app-shell.md B28, B29). A problem `code`
// the app knows gets its own message from the catalog: what happened and what to do. Any other
// answer shows the problem's `title`; no answer at all says the server could not be reached. It
// never signs the user out.
import {
  Button,
  ErrorState,
  type HeadingLevel,
  type Strings,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { ApiError } from "../api";

interface Message {
  title: string;
  description: string;
}

/** The message for a problem `code` the app explains itself, or undefined. */
function knownProblem(code: string | undefined, t: Strings["t"]): Message | undefined {
  switch (code) {
    case "org.none":
      return { title: t("problem.orgNone.title"), description: t("problem.orgNone.description") };
    default:
      return undefined;
  }
}

function messageFor(error: unknown, t: Strings["t"]): Message {
  if (!(error instanceof ApiError)) {
    return { title: t("loadError.unreachableTitle"), description: t("loadError.unreachable") };
  }
  return (
    knownProblem(error.problem?.code, t) ?? {
      title: error.problem?.title ?? t("loadError.title"),
      description: t("loadError.failed"),
    }
  );
}

export interface LoadErrorProps {
  error: unknown;
  level: HeadingLevel;
  onRetry: () => void;
}

export function LoadError({ error, level, onRetry }: LoadErrorProps) {
  const { t } = useStepclimbStrings();
  const { title, description } = messageFor(error, t);
  return (
    <ErrorState
      title={title}
      level={level}
      action={<Button onPress={onRetry}>{t("loadError.retry")}</Button>}
    >
      {description}
    </ErrorState>
  );
}
