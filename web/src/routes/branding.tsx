// SPDX-License-Identifier: AGPL-3.0-or-later
// `/admin/branding`, the organization's branding (docs/specs/m1/16-learning-ui-components.md B14;
// m0/12 B12, B21, B27, Q14): the brand color, fonts, corner style and default appearance, with
// BrandPreview showing the draft brand in light, dark and high contrast and its contrast report as
// the fields change. A draft whose report fails is not sent: Save says so at the brand color. The
// server checks again and answers a refusal field by field, the contrast refusal with its report,
// which the preview then shows. The route is for holders of `branding.manage`; the server refuses
// anyone else, and the page says so in words.
import {
  BrandPreview,
  Button,
  Callout,
  ColorField,
  ErrorState,
  FormError,
  Heading,
  Radio,
  RadioGroup,
  Select,
  SelectItem,
  Skeleton,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useRef, useState } from "react";
import { Form } from "react-aria-components";
import { ApiError } from "../api";
import {
  type Branding,
  type BrandingField,
  brandingQuery,
  brandingRefusal,
  linkSavedStylesheet,
  type SavedInputs,
  saveBranding,
} from "../branding/branding";
import { brandDraft, type Draft, hexOf } from "../branding/draft";
import { useAppContext } from "../context";
import { LoadError } from "../layouts/load-error";
import { meQuery } from "../session";

type Choice<K extends keyof SavedInputs> = SavedInputs[K];

const uiFonts: readonly Choice<"ui_font">[] = ["Atkinson Hyperlegible Next", "Roboto"];
const readingFonts: readonly Choice<"reading_font">[] = ["serif", "sans"];
const cornerStyles: readonly Choice<"corner_style">[] = ["square", "default", "round"];
const appearances: readonly Choice<"default_appearance">[] = [
  "system",
  "light",
  "dark",
  "high-contrast",
];

/** A choice as an ICU select key, which takes no hyphen or space: `high-contrast` -> `highContrast`. */
export function choiceKey(value: string): string {
  const camel = value.replace(/[- ]+(\w)/g, (_, c: string) => c.toUpperCase());
  return camel.charAt(0).toLowerCase() + camel.slice(1);
}

export function BrandingView() {
  const branding = useQuery(brandingQuery);
  if (branding.isError) {
    return (
      <div className="p-4 md:p-6">
        <BrandingLoadError error={branding.error} onRetry={() => void branding.refetch()} />
      </div>
    );
  }
  if (branding.isPending) {
    return (
      <div className="flex flex-col gap-4 p-4 md:p-6">
        <Skeleton className="h-10 w-80 max-w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }
  // A save replaces the saved branding; the form starts again from what the server stored.
  return <BrandingForm key={branding.data.css_hash} saved={branding.data} />;
}

function BrandingLoadError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const { t } = useStepclimbStrings();
  if (error instanceof ApiError && error.status === 403) {
    return (
      <ErrorState title={t("branding.forbidden.title")} level={2}>
        {t("branding.forbidden.description")}
      </ErrorState>
    );
  }
  return <LoadError error={error} level={2} onRetry={onRetry} />;
}

/**
 * The draft of `inputs`, and the one the preview shows: the latest the generator could build, so
 * the preview stays while the color field holds something that is not a color yet.
 */
function useDraft(inputs: SavedInputs): { draft: Draft; shown: Draft } {
  const draft = useMemo(() => brandDraft(inputs), [inputs]);
  const lastGood = useRef(draft);
  if (draft.ok) lastGood.current = draft;
  return { draft, shown: lastGood.current };
}

function BrandingForm({ saved }: { saved: Branding }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const queryClient = useQueryClient();
  const [inputs, setInputs] = useState<SavedInputs>(saved.inputs);
  const [localError, setLocalError] = useState<string | undefined>();
  const set = <K extends keyof SavedInputs>(field: K, value: SavedInputs[K]) => {
    setLocalError(undefined);
    save.reset();
    setInputs((current) => ({ ...current, [field]: value }));
  };
  const { draft, shown } = useDraft(inputs);
  const save = useMutation({
    mutationFn: () => saveBranding(inputs),
    onSuccess: (branding) => {
      linkSavedStylesheet(branding.css_hash);
      queryClient.setQueryData(brandingQuery.queryKey, branding);
      void queryClient.invalidateQueries(meQuery);
      toasts.add({ tone: "success", title: t("branding.saved") });
    },
  });
  const refusal = save.error ? brandingRefusal(save.error, t) : undefined;
  const errors: Partial<Record<BrandingField, string>> = {
    ...refusal?.fields,
    ...(localError === undefined ? {} : { primary: localError }),
  };
  const submit = () => {
    if (save.isPending) return;
    save.reset();
    if (!draft.ok) return setLocalError(t("branding.problem.invalidColor"));
    if (!draft.pass) return setLocalError(t("branding.problem.contrast"));
    setLocalError(undefined);
    save.mutate();
  };
  return (
    <div className="flex flex-col gap-8 p-4 md:p-6">
      <Form
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        validationErrors={errors}
        className="flex max-w-xl flex-col gap-6"
      >
        {saved.fallback ? (
          <Callout kind="warning" title={t("branding.fallback.title")}>
            {t("branding.fallback.description")}
          </Callout>
        ) : null}
        {refusal?.message ? <FormError code={refusal.code}>{refusal.message}</FormError> : null}
        <ColorField
          name="primary"
          label={t("branding.field.primary")}
          description={t("branding.field.primary.description")}
          value={hexOf(inputs.primary) ?? null}
          onChange={(color) => set("primary", color ? color.toString("hex") : "")}
        />
        <Select
          name="ui_font"
          label={t("branding.field.uiFont")}
          value={inputs.ui_font}
          onChange={(value) => set("ui_font", value as Choice<"ui_font">)}
        >
          {uiFonts.map((font) => (
            <SelectItem key={font} id={font}>
              {t("branding.uiFont", { font: choiceKey(font) })}
            </SelectItem>
          ))}
        </Select>
        <RadioGroup
          name="reading_font"
          label={t("branding.field.readingFont")}
          value={inputs.reading_font}
          onChange={(value) => set("reading_font", value as Choice<"reading_font">)}
        >
          {readingFonts.map((font) => (
            <Radio key={font} value={font}>
              {t("branding.readingFont", { font })}
            </Radio>
          ))}
        </RadioGroup>
        <RadioGroup
          name="corner_style"
          label={t("branding.field.cornerStyle")}
          value={inputs.corner_style}
          onChange={(value) => set("corner_style", value as Choice<"corner_style">)}
        >
          {cornerStyles.map((style) => (
            <Radio key={style} value={style}>
              {t("branding.cornerStyle", { style })}
            </Radio>
          ))}
        </RadioGroup>
        <Select
          name="default_appearance"
          label={t("branding.field.defaultAppearance")}
          description={t("branding.field.defaultAppearance.description")}
          value={inputs.default_appearance}
          onChange={(value) => set("default_appearance", value as Choice<"default_appearance">)}
        >
          {appearances.map((appearance) => (
            <SelectItem key={appearance} id={appearance}>
              {t("branding.appearance", { appearance: choiceKey(appearance) })}
            </SelectItem>
          ))}
        </Select>
        <div>
          <Button type="submit" intent="primary" isPending={save.isPending}>
            {t("branding.save")}
          </Button>
        </div>
      </Form>
      <section className="flex flex-col gap-4">
        <Heading level={2} size="lg">
          {t("branding.preview")}
        </Heading>
        {shown.ok ? (
          <BrandPreview themes={shown.themes} reports={refusal?.report ?? shown.reports} />
        ) : null}
      </section>
    </div>
  );
}
