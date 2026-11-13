// SPDX-License-Identifier: AGPL-3.0-or-later
// CopyButton: copies a value with the Clipboard API and says what happened through a live region,
// and in its icon, for a few seconds: copied, or that the copy failed and how to copy it instead
// (docs/specs/m0/13-base-ui-components.md M0 inventory).
import { CheckIcon, CopyIcon, WarningCircleIcon } from "@phosphor-icons/react";
import { useEffect, useState } from "react";
import { composeRenderProps } from "react-aria-components";
import { tv } from "../lib/tv";
import { useStepclimbStrings } from "../strings/strings";
import { IconButton, type IconButtonProps } from "./icon-button";
import { VisuallyHidden } from "./visually-hidden";

export const copyButton = tv({
  base: "",
  variants: {
    result: { idle: "", copied: "text-success-fg", failed: "text-danger-fg" },
  },
});

export type CopyResult = "idle" | "copied" | "failed";

export interface CopyButtonProps
  extends Omit<IconButtonProps, "icon" | "iconState" | "label" | "onPress"> {
  /** The text to copy. */
  value: string;
  /** The accessible name and tooltip; "Copy" from the strings catalog by default. */
  label?: string;
  /** How long the result shows before the button returns to its idle state, in milliseconds. */
  resetAfterMs?: number;
  /** Called with the result of each copy. */
  onCopy?: (result: Exclude<CopyResult, "idle">) => void;
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
    const outcome = (await writeToClipboard(value)) ? "copied" : "failed";
    setResult(outcome);
    onCopy?.(outcome);
  }

  const messages = { idle: "", copied: t("copyButton.copied"), failed: t("copyButton.failed") };
  return (
    <>
      <IconButton
        data-slot="copy-button"
        data-result={result}
        {...props}
        icon={icons[result]}
        iconState={result === "copied" ? "complete" : "available"}
        label={label ?? t("copyButton.label")}
        onPress={copy}
        className={composeRenderProps(className, (cls) => copyButton({ result, className: cls }))}
      />
      <VisuallyHidden data-slot="copy-button-status" role="status">
        {messages[result]}
      </VisuallyHidden>
    </>
  );
}
