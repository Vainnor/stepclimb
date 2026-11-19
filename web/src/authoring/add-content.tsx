// SPDX-License-Identifier: AGPL-3.0-or-later
// Adding a module or an item (docs/specs/m1/16-learning-ui-components.md B19;
// 03-modules-and-items.md B1, B4, B5, B7), each in a side sheet opened from its button: a module
// by its title, an item by its module, kind, title and, for an external link, its address. Both
// are created unpublished and last. Only headers and external links can be created today; the
// sheet says which kinds come later rather than offering them. The sheet stays open with the
// server's refusal, and closes once the board shows the new row, handing focus back to its button.
import {
  type BoardModule,
  Button,
  DialogTrigger,
  FormError,
  Radio,
  RadioGroup,
  Select,
  SelectItem,
  Sheet,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useState } from "react";
import { Form } from "react-aria-components";
import { useAppContext } from "../context";
import { type CreatableKind, createItem, createModule, type NewItem } from "./course-content";
import { type AuthoringProblem, authoringProblem } from "./problems";
import { useContentWrite } from "./use-content-write";

function at(problem: AuthoringProblem | undefined, field: string): string | undefined {
  return problem?.field === field ? problem.message : undefined;
}

/** The general refusal, when it names no field of the form. */
function GeneralProblem({ problem }: { problem: AuthoringProblem | undefined }) {
  if (problem === undefined || problem.field !== undefined) return null;
  return <FormError code={problem.code}>{problem.message}</FormError>;
}

/** The form's last row: Cancel and the action that ends it. */
function Actions({
  close,
  submit,
  pending,
}: {
  close: () => void;
  submit: string;
  pending: boolean;
}) {
  const { t } = useStepclimbStrings();
  return (
    <div className="flex flex-wrap justify-end gap-2">
      <Button onPress={close}>{t("authoring.cancel")}</Button>
      <Button type="submit" intent="primary" isPending={pending}>
        {submit}
      </Button>
    </div>
  );
}

export function AddModule({ courseId }: { courseId: string }) {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button intent="primary">{t("authoring.module.add")}</Button>
      <Sheet title={t("authoring.module.add.title")}>
        {({ close }) => <AddModuleForm courseId={courseId} close={close} />}
      </Sheet>
    </DialogTrigger>
  );
}

function AddModuleForm({ courseId, close }: { courseId: string; close: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [title, setTitle] = useState("");
  const create = useContentWrite(courseId, (value: string) => createModule(courseId, value));
  const problem = create.error ? authoringProblem(create.error, t, ["title"]) : undefined;
  return (
    <Form
      aria-label={t("authoring.module.add.title")}
      className="flex flex-col gap-5"
      onSubmit={(event) => {
        event.preventDefault();
        if (create.isPending) return;
        create.mutate(title, {
          onSuccess: (module) => {
            toasts.add({ tone: "success", title: t("authoring.added", { title: module.title }) });
            close();
          },
        });
      }}
    >
      <GeneralProblem problem={problem} />
      <TextField
        label={t("authoring.field.title")}
        description={t("authoring.module.add.description")}
        value={title}
        onChange={(value) => {
          create.reset();
          setTitle(value);
        }}
        isRequired
        autoFocus
        isInvalid={at(problem, "title") !== undefined}
        errorMessage={at(problem, "title")}
      />
      <Actions close={close} submit={t("authoring.module.add.submit")} pending={create.isPending} />
    </Form>
  );
}

interface AddItemProps {
  courseId: string;
  modules: readonly BoardModule[];
}

export function AddItem({ courseId, modules }: AddItemProps) {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button isDisabled={modules.length === 0}>{t("authoring.item.add")}</Button>
      <Sheet title={t("authoring.item.add.title")}>
        {({ close }) => <AddItemForm courseId={courseId} modules={modules} close={close} />}
      </Sheet>
    </DialogTrigger>
  );
}

interface ItemDraft {
  moduleId: string;
  kind: CreatableKind;
  title: string;
  url: string;
}

function newItem(draft: ItemDraft): NewItem {
  return draft.kind === "external_link"
    ? { kind: "external_link", title: draft.title, url: draft.url }
    : { kind: "header", title: draft.title };
}

function AddItemForm({ courseId, modules, close }: AddItemProps & { close: () => void }) {
  const { t } = useStepclimbStrings();
  const { toasts } = useAppContext();
  const [draft, setDraft] = useState<ItemDraft>({
    moduleId: modules.at(-1)?.id ?? "",
    kind: "external_link",
    title: "",
    url: "",
  });
  const create = useContentWrite(courseId, (value: ItemDraft) =>
    createItem(value.moduleId, newItem(value)),
  );
  // An edit answers the last refusal: the field it marked takes new input.
  const set = (change: Partial<ItemDraft>) => {
    create.reset();
    setDraft((d) => ({ ...d, ...change }));
  };
  // The address field is drawn only for an external link.
  const drawn = draft.kind === "external_link" ? ["title", "url"] : ["title"];
  const problem = create.error ? authoringProblem(create.error, t, drawn) : undefined;
  return (
    <Form
      aria-label={t("authoring.item.add.title")}
      className="flex flex-col gap-5"
      onSubmit={(event) => {
        event.preventDefault();
        if (create.isPending) return;
        create.mutate(draft, {
          onSuccess: (item) => {
            toasts.add({ tone: "success", title: t("authoring.added", { title: item.title }) });
            close();
          },
        });
      }}
    >
      <GeneralProblem problem={problem} />
      <Select
        label={t("authoring.field.module")}
        value={draft.moduleId}
        onChange={(key) => set({ moduleId: String(key) })}
        isRequired
      >
        {modules.map((m, index) => (
          <SelectItem key={m.id} id={m.id}>
            {t("moduleBoard.moduleName", { number: index + 1, title: m.title })}
          </SelectItem>
        ))}
      </Select>
      <RadioGroup
        label={t("authoring.field.kind")}
        value={draft.kind}
        onChange={(kind) => set({ kind: kind === "header" ? "header" : "external_link" })}
      >
        <Radio value="external_link" description={t("authoring.kind.externalLink.description")}>
          {t("itemRow.kind", { kind: "external_link" })}
        </Radio>
        <Radio value="header" description={t("authoring.kind.header.description")}>
          {t("moduleBoard.header")}
        </Radio>
      </RadioGroup>
      <Text elementType="p" size="sm" tone="sub">
        {t("authoring.kind.later")}
      </Text>
      <TextField
        label={t("authoring.field.title")}
        value={draft.title}
        onChange={(title) => set({ title })}
        isRequired
        isInvalid={at(problem, "title") !== undefined}
        errorMessage={at(problem, "title")}
      />
      {draft.kind === "external_link" ? (
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
      <Actions close={close} submit={t("authoring.item.add.submit")} pending={create.isPending} />
    </Form>
  );
}
