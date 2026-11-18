// SPDX-License-Identifier: AGPL-3.0-or-later
// A module's editor in ModuleBoard's side sheet (docs/specs/m1/16-learning-ui-components.md B19;
// 03-modules-and-items.md B1 to B3; 04-unlock-and-completion-rules.md B1, B2): its title, whether
// learners see it, and its rules: the date it opens, the modules it waits for, whether its items
// go in order, and whether all or one of its requirements complete it. Save sends only what
// changed, in one `PATCH`; the server refuses a prerequisite cycle. Delete asks first, and the
// server refuses a module that still holds items.
import { fromDate, type ZonedDateTime } from "@internationalized/date";
import {
  AlertDialog,
  type BoardModule,
  Button,
  Checkbox,
  CheckboxGroup,
  DatePicker,
  FormError,
  Radio,
  RadioGroup,
  Separator,
  Switch,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useState } from "react";
import { Form } from "react-aria-components";
import { useAppContext } from "../context";
import { deleteModule, type Module, type ModulePatch, updateModule } from "./course-content";
import { type AuthoringProblem, authoringProblem } from "./problems";
import { useContentWrite } from "./use-content-write";

interface ModuleEditorProps {
  courseId: string;
  module: Module;
  /** The course's modules in order, for the prerequisites. */
  modules: readonly BoardModule[];
  timeZone: string;
  close: () => void;
}

interface Draft {
  title: string;
  published: boolean;
  unlockAt: ZonedDateTime | null;
  prerequisiteIds: string[];
  requireSequential: boolean;
  completionRule: "all" | "one";
}

function draftOf(module: Module, timeZone: string): Draft {
  return {
    title: module.title,
    published: module.published,
    unlockAt: module.unlock_at ? fromDate(new Date(module.unlock_at), timeZone) : null,
    prerequisiteIds: [...module.prerequisite_ids],
    requireSequential: module.require_sequential,
    completionRule: module.completion_rule,
  };
}

function sameSet(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id) => b.includes(id));
}

function instant(value: ZonedDateTime | null): number | null {
  return value === null ? null : value.toDate().getTime();
}

/** The members of `draft` that differ from `module`, as the `PATCH` body. */
export function modulePatch(module: Module, draft: Draft): ModulePatch {
  const patch: ModulePatch = {};
  if (draft.title !== module.title) patch.title = draft.title;
  if (draft.published !== module.published) patch.published = draft.published;
  const before = module.unlock_at ? new Date(module.unlock_at).getTime() : null;
  if (instant(draft.unlockAt) !== before) {
    patch.unlock_at = draft.unlockAt === null ? null : draft.unlockAt.toDate().toISOString();
  }
  if (!sameSet(draft.prerequisiteIds, module.prerequisite_ids)) {
    patch.prerequisite_ids = draft.prerequisiteIds;
  }
  if (draft.requireSequential !== module.require_sequential) {
    patch.require_sequential = draft.requireSequential;
  }
  if (draft.completionRule !== module.completion_rule) {
    patch.completion_rule = draft.completionRule;
  }
  return patch;
}

/** The message for `field`, when the problem names it. */
function at(problem: AuthoringProblem | undefined, field: string): string | undefined {
  return problem?.field === field ? problem.message : undefined;
}

export function ModuleEditor({ courseId, module, modules, timeZone, close }: ModuleEditorProps) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [draft, setDraft] = useState(() => draftOf(module, timeZone));
  const save = useContentWrite(courseId, (patch: ModulePatch) => updateModule(module.id, patch));
  // An edit answers the last refusal: the field it marked takes new input.
  const set = (change: Partial<Draft>) => {
    save.reset();
    setDraft((d) => ({ ...d, ...change }));
  };
  const problem = save.error ? authoringProblem(save.error, t) : undefined;
  const others = modules.filter((m) => m.id !== module.id);
  const submit = () => {
    const patch = modulePatch(module, draft);
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
      <Form
        aria-label={t("authoring.module.form")}
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
        <Switch isSelected={draft.published} onChange={(published) => set({ published })}>
          {t("authoring.field.published")}
        </Switch>
        <UnlockDate
          value={draft.unlockAt}
          onChange={(unlockAt) => set({ unlockAt })}
          timeZone={timeZone}
          error={at(problem, "unlock_at")}
        />
        {others.length === 0 ? null : (
          <CheckboxGroup
            label={t("authoring.field.prerequisites")}
            description={t("authoring.field.prerequisites.description")}
            value={draft.prerequisiteIds}
            onChange={(prerequisiteIds) => set({ prerequisiteIds })}
            isInvalid={at(problem, "prerequisite_ids") !== undefined}
            errorMessage={at(problem, "prerequisite_ids")}
          >
            {others.map((m) => (
              <Checkbox key={m.id} value={m.id}>
                {t("moduleBoard.moduleName", { number: modules.indexOf(m) + 1, title: m.title })}
              </Checkbox>
            ))}
          </CheckboxGroup>
        )}
        <Switch
          isSelected={draft.requireSequential}
          onChange={(requireSequential) => set({ requireSequential })}
          description={t("authoring.field.sequential.description")}
        >
          {t("authoring.field.sequential")}
        </Switch>
        <RadioGroup
          label={t("authoring.field.completion")}
          value={draft.completionRule}
          onChange={(value) => set({ completionRule: value === "one" ? "one" : "all" })}
        >
          <Radio value="all">{t("authoring.field.completion.all")}</Radio>
          <Radio value="one">{t("authoring.field.completion.one")}</Radio>
        </RadioGroup>
        <div className="flex flex-wrap justify-end gap-2">
          <Button onPress={close}>{t("authoring.cancel")}</Button>
          <Button type="submit" intent="primary" isPending={save.isPending}>
            {t("authoring.save")}
          </Button>
        </div>
      </Form>
      <Separator />
      <DeleteModule courseId={courseId} module={module} close={close} />
    </div>
  );
}

interface UnlockDateProps {
  value: ZonedDateTime | null;
  onChange: (value: ZonedDateTime | null) => void;
  timeZone: string;
  error: string | undefined;
}

/** The date the module opens, with a button that takes it away. */
function UnlockDate({ value, onChange, timeZone, error }: UnlockDateProps) {
  const { t } = useStepclimbStrings();
  return (
    <div className="flex flex-col items-start gap-2">
      <DatePicker
        label={t("authoring.field.unlockAt")}
        description={t("authoring.field.unlockAt.description")}
        timeZone={timeZone}
        value={value}
        onChange={onChange}
        isInvalid={error !== undefined}
        errorMessage={error}
      />
      {value === null ? null : (
        <Button onPress={() => onChange(null)}>{t("authoring.field.unlockAt.clear")}</Button>
      )}
    </div>
  );
}

function DeleteModule({
  courseId,
  module,
  close,
}: {
  courseId: string;
  module: Module;
  close: () => void;
}) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [asking, setAsking] = useState(false);
  const remove = useContentWrite(courseId, () => deleteModule(module.id));
  const problem = remove.error ? authoringProblem(remove.error, t) : undefined;
  return (
    <div className="flex flex-col items-start gap-3">
      {problem ? <FormError code={problem.code}>{problem.message}</FormError> : null}
      <Button intent="danger" onPress={() => setAsking(true)} isPending={remove.isPending}>
        {t("authoring.module.delete")}
      </Button>
      <AlertDialog
        isOpen={asking}
        onOpenChange={setAsking}
        title={t("authoring.module.delete.title", { title: module.title })}
        confirmLabel={t("authoring.module.delete")}
        intent="danger"
        onConfirm={() =>
          remove.mutate(undefined, {
            onSuccess: () => {
              toasts.add({
                tone: "success",
                title: t("authoring.deleted", { title: module.title }),
              });
              close();
            },
          })
        }
      >
        {t("authoring.module.delete.description")}
      </AlertDialog>
    </div>
  );
}
