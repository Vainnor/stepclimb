// SPDX-License-Identifier: AGPL-3.0-or-later
// CopyButton: copies a value with the Clipboard API and says what happened for a few seconds,
// through a live region and in its icon: copied, or that the copy failed and how to copy it
// instead, which also shows as text beside the button (docs/specs/m0/13-base-ui-components.md M0
// inventory).
import { CheckIcon, CopyIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { IconButton, type IconButtonProps } from "./icon-button";

export const copyButton = tv({
  slots: {
    root: "inline-flex items-center gap-2",
    button: "",
    status: "sr-only",
  },
  variants: {
    result: {
      idle: {},
      copied: { button: "text-success-fg" },
      failed: {
        button: "text-danger-fg",
        status: "not-sr-only font-ui text-caption text-danger-fg",
      },
    },
  },
});

export type CopyResult = "idle" | "copied" | "failed";

export interface CopyButtonProps
  extends Omit<IconButtonProps, "icon" | "iconState" | "label" | "onPress" | "className"> {
  /** The text to copy. */
  value: string;
  /** The accessible name and tooltip; "Copy" from the strings catalog by default. */
  label?: string;
  /** How long the result shows before the button returns to its idle state, in milliseconds. */
  resetAfterMs?: number;
  /** Called with the result of each copy. */
  onCopy?: (result: Exclude<CopyResult, "idle">) => void;
  /** Layout only, on the root around the button and its message. */
  className?: string;
}

const icons = { idle: CopyIcon, copied: CheckIcon, failed: WarningCircleIcon } as const;

async function writeToClipboard(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    // The browser refused (no permission, or the document does not have focus), or the page is
    // not a secure context and has no `navigator.clipboard`, so the call above throws. The live
    // region tells the person; there is nothing else to recover.
    return false;
  }
}

export function CopyButton({
  value,
  label,
  resetAfterMs = 2000,
  onCopy,
  className,
  ...props
}: CopyButtonProps) {
  const { t } = useStepclimbStrings();
  const [result, setResult] = useState<CopyResult>("idle");

  useEffect(() => {
    if (result === "idle") return;
    const timer = setTimeout(() => setResult("idle"), resetAfterMs);
    return () => clearTimeout(timer);
  }, [result, resetAfterMs]);

  async function copy() {
    // Empty the live region first, so a second copy with the same result is announced again.
    setResult("idle");
    const outcome = (await writeToClipboard(value)) ? "copied" : "failed";
    setResult(outcome);
    onCopy?.(outcome);
  }

  const messages = { idle: "", copied: t("copyButton.copied"), failed: t("copyButton.failed") };
  const styles = copyButton({ result });
  return (
    <span data-slot="copy-button" data-result={result} className={styles.root({ className })}>
      <IconButton
        data-slot="copy-button-button"
        {...props}
        icon={icons[result]}
        iconState={result === "copied" ? "complete" : "available"}
        label={label ?? t("copyButton.label")}
        onPress={copy}
        className={styles.button()}
      />
      <span data-slot="copy-button-status" role="status" className={styles.status()}>
        {messages[result]}
      </span>
    </span>
  );
}
