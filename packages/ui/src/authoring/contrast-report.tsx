// SPDX-License-Identifier: AGPL-3.0-or-later
// ContrastReport: the theme generator's contrast report for a brand, said in words
// (docs/specs/m1/16-learning-ui-components.md B14; m0/12 B20, B21). A summary says whether the
// colors can be saved; a tab per appearance lists every measured pair, failing pairs first, with
// its ratio, the minimum and the result as an icon and a word; the generator's warnings follow.
// The report is the generator's (`@stepclimb/tokens/report`) or the server's answer, which has
// the same shape.
import { CheckCircleIcon, XCircleIcon } from "@phosphor-icons/react";
import type { Appearance } from "@stepclimb/tokens/generate";
import { Callout } from "../learning/callout";
import { tv } from "../lib/tv";
import { Cell, Column, Row, Table, TableBody, TableHeader } from "../primitives/table";
import { Tab, TabList, TabPanel, Tabs } from "../primitives/tabs";
import { Text } from "../primitives/text";
import { useStepclimbStrings } from "../strings/strings";
import { Icon } from "../theme/icon";

export const contrastReport = tv({
  slots: {
    root: "flex min-w-0 flex-col gap-4 font-ui",
    summary: "flex items-start gap-2 text-base",
    summaryIcon: "mt-0.5 shrink-0",
    result: "inline-flex items-center gap-1.5",
  },
  variants: {
    pass: {
      true: { summaryIcon: "text-success-fg", result: "text-success-fg" },
      false: { summaryIcon: "text-danger-fg", result: "text-danger-fg font-medium" },
    },
  },
});

/** One appearance's report, as the generator gives it and the server answers it. */
export interface ContrastReportEntry {
  appearance: Appearance;
  /** True when every pair passes. */
  pass: boolean;
  pairs: readonly {
    foreground: string;
    background: string;
    /** The WCAG 2 contrast ratio, unrounded. */
    ratio: number;
    minimum: number;
    pass: boolean;
  }[];
  /** Things to look at that never fail the theme. */
  warnings: readonly { code: string; message: string }[];
}

export interface ContrastReportProps {
  /** One report per appearance, in the order the tabs show them. */
  reports: readonly ContrastReportEntry[];
  /** Layout only. */
  className?: string;
}

/** An appearance as an ICU select key, which takes no hyphen. */
export function appearanceKey(appearance: Appearance): string {
  return appearance === "high-contrast" ? "highContrast" : appearance;
}

/** The ratio cut, not rounded, to two decimals, so a failing 4.496 never reads as 4.50. */
function shownRatio(ratio: number): number {
  return Math.floor(ratio * 100) / 100;
}

function failingCount(report: ContrastReportEntry): number {
  return report.pairs.filter((pair) => !pair.pass).length;
}

/** Failing pairs first, each group in the report's own order. */
function ordered(report: ContrastReportEntry): ContrastReportEntry["pairs"] {
  return [...report.pairs.filter((p) => !p.pass), ...report.pairs.filter((p) => p.pass)];
}

function Result({ pass }: { pass: boolean }) {
  const { t } = useStepclimbStrings();
  const styles = contrastReport({ pass });
  return (
    <span data-slot="contrast-report-result" data-pass={pass} className={styles.result()}>
      <Icon icon={pass ? CheckCircleIcon : XCircleIcon} size="sm" />
      {pass ? t("contrastReport.pass") : t("contrastReport.fail")}
    </span>
  );
}

function PairTable({ report }: { report: ContrastReportEntry }) {
  const { t } = useStepclimbStrings();
  const appearance = t("brandPreview.appearance", { appearance: appearanceKey(report.appearance) });
  return (
    <Table aria-label={appearance}>
      <TableHeader>
        <Column isRowHeader>{t("contrastReport.column.pair")}</Column>
        <Column align="end">{t("contrastReport.column.contrast")}</Column>
        <Column align="end">{t("contrastReport.column.needs")}</Column>
        <Column>{t("contrastReport.column.result")}</Column>
      </TableHeader>
      <TableBody>
        {ordered(report).map((pair) => (
          <Row
            key={`${pair.foreground} ${pair.background}`}
            id={`${pair.foreground} ${pair.background}`}
          >
            <Cell>
              {t("contrastReport.pair", {
                foreground: pair.foreground,
                background: pair.background,
              })}
            </Cell>
            <Cell align="end">{t("contrastReport.ratio", { ratio: shownRatio(pair.ratio) })}</Cell>
            <Cell align="end">{t("contrastReport.minimum", { minimum: pair.minimum })}</Cell>
            <Cell>
              <Result pass={pair.pass} />
            </Cell>
          </Row>
        ))}
      </TableBody>
    </Table>
  );
}

/** A warning in the catalog's words; an unknown code shows the generator's own message. */
function Warning({ code, message }: { code: string; message: string }) {
  const { t } = useStepclimbStrings();
  const text = code === "brand-near-danger" ? t("contrastReport.warning.brandNearDanger") : message;
  return (
    <Callout kind="warning" className="max-w-prose">
      {text}
    </Callout>
  );
}

/** The appearance whose tab opens first: the first that fails, else the first. */
function firstTab(reports: readonly ContrastReportEntry[]): Appearance | undefined {
  return (reports.find((r) => !r.pass) ?? reports[0])?.appearance;
}

export function ContrastReport({ reports, className }: ContrastReportProps) {
  const { t } = useStepclimbStrings();
  const failing = reports.reduce((sum, report) => sum + failingCount(report), 0);
  const pass = failing === 0;
  const styles = contrastReport({ pass });
  const warnings = new Map(
    reports.flatMap((report) => report.warnings.map((w) => [w.code, w] as const)),
  );
  return (
    <section
      data-slot="contrast-report"
      data-pass={pass}
      aria-label={t("contrastReport.label")}
      className={styles.root({ className })}
    >
      <p data-slot="contrast-report-summary" role="status" className={styles.summary()}>
        <Icon
          icon={pass ? CheckCircleIcon : XCircleIcon}
          size="md"
          className={styles.summaryIcon()}
        />
        <Text>
          {pass
            ? t("contrastReport.summary.pass")
            : t("contrastReport.summary.fail", { count: failing })}
        </Text>
      </p>
      {[...warnings.values()].map((warning) => (
        <Warning key={warning.code} code={warning.code} message={warning.message} />
      ))}
      <Tabs key={firstTab(reports)} defaultSelectedKey={firstTab(reports)}>
        <TabList aria-label={t("contrastReport.tabs")}>
          {reports.map((report) => (
            <Tab key={report.appearance} id={report.appearance}>
              {t("contrastReport.appearance", {
                appearance: appearanceKey(report.appearance),
                failing: failingCount(report),
              })}
            </Tab>
          ))}
        </TabList>
        {reports.map((report) => (
          <TabPanel key={report.appearance} id={report.appearance}>
            <PairTable report={report} />
          </TabPanel>
        ))}
      </Tabs>
    </section>
  );
}
