// SPDX-License-Identifier: AGPL-3.0-or-later
// An item's editor in ModuleBoard's side sheet (docs/specs/m1/16-learning-ui-components.md B19;
// 03-modules-and-items.md B4 to B9; 04-unlock-and-completion-rules.md B3): its title, an external
// link's address, whether learners see it and what completes it. Save sends only what changed, in
// one `PATCH`. A header takes no requirement (`03` B8). A page item links to the page editor.
// Delete asks first, and the server refuses an item a learner has made progress on.
import {
  AlertDialog,
  Button,
  ButtonLink,
  FormError,
  NumberField,
  Radio,
  RadioGroup,
  type Requirement,
  Separator,
  Switch,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useRouter } from "@tanstack/react-router";
import { useState } from "react";
import { Form } from "react-aria-components";
import { useAppContext } from "../context";
import { deleteItem, type Item, type ItemPatch, updateItem } from "./course-content";
import { type AuthoringProblem, authoringProblem } from "./problems";
import { useContentWrite } from "./use-content-write";

type RequirementKind = "none" | "view" | "mark_done" | "watch";

interface Draft {
  title: string;
  url: string;
  published: boolean;
  requirement: RequirementKind;
  /** `watch`'s share of the video, 1 to 100. */
  minPercent: number;
}

/** The share a new `watch` requirement starts at (`ui` RequirementLabel's "Watch 80%"). */
const DEFAULT_WATCH_PERCENT = 80;

function draftOf(item: Item): Draft {
  const r = item.requirement ?? null;
  return {
    title: item.title,
    url: item.url ?? "",
    published: item.published,
    requirement: r === null ? "none" : r.kind,
    minPercent: r?.kind === "watch" ? r.min_percent : DEFAULT_WATCH_PERCENT,
  };
}

function requirementOf(draft: Draft): Requirement | null {
  switch (draft.requirement) {
    case "none":
      return null;
    case "watch":
      return { kind: "watch", min_percent: draft.minPercent };
    default:
      return { kind: draft.requirement };
  }
}

function sameRequirement(a: Requirement | null, b: Requirement | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The members of `draft` that differ from `item`, as the `PATCH` body. */
export function itemPatch(item: Item, draft: Draft): ItemPatch {
  const patch: ItemPatch = {};
  if (draft.title !== item.title) patch.title = draft.title;
  if (item.kind === "external_link" && draft.url !== (item.url ?? "")) patch.url = draft.url;
  if (draft.published !== item.published) patch.published = draft.published;
  const requirement = requirementOf(draft);
  if (item.kind !== "header" && !sameRequirement(requirement, item.requirement ?? null)) {
    patch.requirement = requirement;
  }
  return patch;
}

/** The request members the item's form has a field for, where a refusal of one is shown. */
function drawnFields(item: Item): string[] {
  const fields = ["title"];
  if (item.kind === "external_link") fields.push("url");
  if (item.kind !== "header") fields.push("requirement");
  return fields;
}

function at(problem: AuthoringProblem | undefined, field: string): string | undefined {
  return problem?.field === field ? problem.message : undefined;
}

interface ItemEditorProps {
  courseId: string;
  item: Item;
  close: () => void;
}

export function ItemEditor({ courseId, item, close }: ItemEditorProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [draft, setDraft] = useState(() => draftOf(item));
  const save = useContentWrite(courseId, (patch: ItemPatch) => updateItem(item.id, patch));
  // An edit answers the last refusal: the field it marked takes new input.
  const set = (change: Partial<Draft>) => {
    save.reset();
    setDraft((d) => ({ ...d, ...change }));
  };
  const problem = save.error ? authoringProblem(save.error, t, drawnFields(item)) : undefined;
  const submit = () => {
    const patch = itemPatch(item, draft);
    if (Object.keys(patch).length === 0) return close();
    save.mutate(patch, {
      onSuccess: (saved) => {
        toasts.add({ tone: "success", title: t("authoring.saved", { title: saved.title }) });
        close();
      },
    });
  };
  return (
    <div className="flex flex-col gap-6">
      {item.kind === "page" && item.ref_id ? (
        <PageLink courseId={courseId} pageId={item.ref_id} />
      ) : null}
      <Form
        aria-label={t("authoring.item.form")}
        className="flex flex-col gap-5"
        onSubmit={(event) => {
          event.preventDefault();
          if (!save.isPending) submit();
        }}
      >
        {problem && problem.field === undefined ? (
          <FormError code={problem.code}>{problem.message}</FormError>
        ) : null}
        <TextField
          label={t("authoring.field.title")}
          value={draft.title}
          onChange={(title) => set({ title })}
          isRequired
          isInvalid={at(problem, "title") !== undefined}
          errorMessage={at(problem, "title")}
        />
        {item.kind === "external_link" ? (
          <TextField
            label={t("authoring.field.url")}
            description={t("authoring.field.url.description")}
            type="url"
            value={draft.url}
            onChange={(url) => set({ url })}
            isRequired
            isInvalid={at(problem, "url") !== undefined}
            errorMessage={at(problem, "url")}
          />
        ) : null}
        <Switch isSelected={draft.published} onChange={(published) => set({ published })}>
          {t("authoring.field.published")}
        </Switch>
        {item.kind === "header" ? null : (
          <RequirementField
            kind={item.kind}
            draft={draft}
            set={set}
            error={at(problem, "requirement")}
          />
        )}
        <div className="flex flex-wrap justify-end gap-2">
          <Button onPress={close}>{t("authoring.cancel")}</Button>
          <Button type="submit" intent="primary" isPending={save.isPending}>
            {t("authoring.save")}
          </Button>
        </div>
      </Form>
      <Separator />
      <DeleteItem courseId={courseId} item={item} close={close} />
    </div>
  );
}

interface RequirementFieldProps {
  kind: Item["kind"];
  draft: Draft;
  set: (change: Partial<Draft>) => void;
  error: string | undefined;
}

/** What completes the item: the kinds `04` B3 allows on it. */
function RequirementField({ kind, draft, set, error }: RequirementFieldProps) {
  const { t } = useStepclimbStrings();
  return (
    <div className="flex flex-col gap-3">
      <RadioGroup
        label={t("authoring.field.requirement")}
        value={draft.requirement}
        onChange={(value) => set({ requirement: value as RequirementKind })}
        isInvalid={error !== undefined}
        errorMessage={error}
      >
        <Radio value="none">{t("authoring.field.requirement.none")}</Radio>
        <Radio value="view">{t("authoring.field.requirement.view")}</Radio>
        <Radio value="mark_done">{t("authoring.field.requirement.markDone")}</Radio>
        {kind === "video" ? (
          <Radio value="watch">{t("authoring.field.requirement.watch")}</Radio>
        ) : null}
      </RadioGroup>
      {draft.requirement === "watch" ? (
        <NumberField
          label={t("authoring.field.minPercent")}
          minValue={1}
          maxValue={100}
          step={1}
          value={draft.minPercent}
          onChange={(minPercent) => set({ minPercent })}
        />
      ) : null}
    </div>
  );
}

function PageLink({ courseId, pageId }: { courseId: string; pageId: string }) {
  const { t } = useStepclimbStrings();
  const router = useRouter();
  const href = router.buildLocation({
    to: "/c/$courseId/pages/$pageId/edit",
    params: { courseId, pageId },
  }).href;
  return (
    <ButtonLink href={href} className="self-start">
      {t("authoring.item.editPage")}
    </ButtonLink>
  );
}

function DeleteItem({ courseId, item, close }: ItemEditorProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [asking, setAsking] = useState(false);
  const remove = useContentWrite(courseId, () => deleteItem(item.id));
  const problem = remove.error ? authoringProblem(remove.error, t) : undefined;
  return (
    <div className="flex flex-col items-start gap-3">
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Button intent="danger" onPress={() => setAsking(true)} isPending={remove.isPending}>
        {t("authoring.item.delete")}
      </Button>
      <AlertDialog
        isOpen={asking}
        onOpenChange={setAsking}
        title={t("authoring.item.delete.title", { title: item.title })}
        confirmLabel={t("authoring.item.delete")}
        intent="danger"
        onConfirm={() =>
          remove.mutate(undefined, {
            onSuccess: () => {
              toasts.add({ tone: "success", title: t("authoring.deleted", { title: item.title }) });
              close();
            },
          })
        }
      >
        {t("authoring.item.delete.description")}
      </AlertDialog>
    </div>
  );
}
