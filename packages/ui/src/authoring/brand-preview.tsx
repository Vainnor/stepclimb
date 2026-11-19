// SPDX-License-Identifier: AGPL-3.0-or-later
// BrandPreview: a draft brand before it is saved (docs/specs/m1/16-learning-ui-components.md
// B14; m0/12 B21). A sample screen (a top bar, a course row, a button, a callout and text) drawn
// in the draft's light, dark and high-contrast themes side by side, then the generator's contrast
// report for them. Each sample takes the draft's tokens as custom properties on its own element
// (`themeDeclarations`), so it shows the draft whatever brand the page around it is drawn in. The
// samples are pictures, not controls: they are inert, out of the tab order and the accessibility
// tree, and the report says in words what they show.
import {
  type Appearance,
  type GeneratedThemes,
  themeDeclarations,
} from "@stepclimb/tokens/generate";
import type { CSSProperties } from "react";
import { Callout } from "../learning/callout";
import { StatusChip } from "../learning/status-chip";
import { tv } from "../lib/tv";
import { Button } from "../primitives/button";
import { Link } from "../primitives/link";
import { ProgressBar } from "../primitives/progress-bar";
import { Text } from "../primitives/text";
import { useStepclimbStrings } from "../strings/strings";
import { appearanceKey, ContrastReport, type ContrastReportEntry } from "./contrast-report";

export const brandPreview = tv({
  slots: {
    root: "flex min-w-0 flex-col gap-6 font-ui",
    samples: "grid grid-cols-1 gap-4 lg:grid-cols-3",
    figure: "flex min-w-0 flex-col gap-2",
    caption: "text-sm font-medium text-fg-sub",
    // The sample's own tokens draw its frame: a line in the draft's color, on its canvas.
    sample:
      "flex min-w-0 flex-col overflow-hidden rounded-md border border-line bg-canvas text-fg-default",
    topBar: "flex items-center border-b border-line bg-raised px-4 py-3 font-medium text-fg-strong",
    body: "flex flex-col gap-4 p-4",
    courseRow: "flex flex-col gap-3 rounded-md border border-line bg-raised p-3",
    courseTitle: "flex flex-wrap items-center justify-between gap-2 font-medium text-fg-strong",
  },
});

/** The order the samples and the report's tabs show the appearances in. */
const shownAppearances: readonly Appearance[] = ["light", "dark", "high-contrast"];

export interface BrandPreviewProps {
  /** The draft brand's themes, from the generator (`generate` of `@stepclimb/tokens/generate`). */
  themes: GeneratedThemes;
  /**
   * The contrast report to show: the generator's for `themes`, or the server's when it refused
   * the brand.
   */
  reports: readonly ContrastReportEntry[];
  /** Layout only. */
  className?: string;
}

/** The theme's tokens as inline custom properties. */
function themeStyle(themes: GeneratedThemes, appearance: Appearance): CSSProperties {
  return Object.fromEntries(themeDeclarations(themes[appearance])) as CSSProperties;
}

function Sample({ themes, appearance }: { themes: GeneratedThemes; appearance: Appearance }) {
  const { t } = useStepclimbStrings();
  const styles = brandPreview();
  return (
    <figure data-slot="brand-preview-figure" className={styles.figure()}>
      <figcaption data-slot="brand-preview-caption" className={styles.caption()}>
        {t("brandPreview.appearance", { appearance: appearanceKey(appearance) })}
      </figcaption>
      <div
        data-slot="brand-preview-sample"
        data-sc-theme={appearance}
        data-appearance={appearance}
        inert
        style={themeStyle(themes, appearance)}
        className={styles.sample()}
      >
        <div data-slot="brand-preview-top-bar" className={styles.topBar()}>
          {t("brandPreview.sample.organization")}
        </div>
        <div className={styles.body()}>
          <div data-slot="brand-preview-course" className={styles.courseRow()}>
            <div className={styles.courseTitle()}>
              <span>{t("brandPreview.sample.course")}</span>
              <StatusChip state="progress" />
            </div>
            <ProgressBar label={t("brandPreview.sample.progress")} value={40} />
          </div>
          <div>
            <Button intent="primary">{t("brandPreview.sample.button")}</Button>
          </div>
          <Callout kind="info">{t("brandPreview.sample.callout")}</Callout>
          <Text>{t("brandPreview.sample.text")}</Text>
          <Link>{t("brandPreview.sample.link")}</Link>
        </div>
      </div>
    </figure>
  );
}

export function BrandPreview({ themes, reports, className }: BrandPreviewProps) {
  const { t } = useStepclimbStrings();
  const styles = brandPreview();
  return (
    <section
      data-slot="brand-preview"
      aria-label={t("brandPreview.label")}
      className={styles.root({ className })}
    >
      <div data-slot="brand-preview-samples" className={styles.samples()}>
        {shownAppearances.map((appearance) => (
          <Sample key={appearance} themes={themes} appearance={appearance} />
        ))}
      </div>
      <ContrastReport reports={reports} />
    </section>
  );
}
