// SPDX-License-Identifier: AGPL-3.0-or-later
// Joins short phrases as the locale lists units ("Page, Mark as done, Available" in English):
// commas, with no "and", for a description read as one run.
import { useCallback, useMemo } from "react";
import { useLocale } from "react-aria-components";

export function useUnitList(): (texts: readonly string[]) => string {
  const { locale } = useLocale();
  const units = useMemo(
    () => new Intl.ListFormat(locale, { style: "long", type: "unit" }),
    [locale],
  );
  return useCallback((texts) => units.format(texts), [units]);
}
