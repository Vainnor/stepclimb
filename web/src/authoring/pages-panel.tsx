// SPDX-License-Identifier: AGPL-3.0-or-later
// The course's pages on the authoring screen (docs/specs/m1/16-learning-ui-components.md B19;
// 07-pages-and-the-editor.md B1, B21): each page links to its editor, and "New page" creates one
// from its title and opens it in the editor. A page cannot be put in a module until `page` items
// can be created (`07` B19), which the panel says.
import {
  Button,
  DialogTrigger,
  FormError,
  Heading,
  Link,
  Sheet,
  Skeleton,
  Text,
  TextField,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useInfiniteQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { useId, useState } from "react";
import { Form } from "react-aria-components";
import { ApiError } from "../api";
import { LoadError } from "../layouts/load-error";
import { createPage, type PageSummary, pagesKey, pagesQuery } from "./course-content";
import { authoringProblem } from "./problems";

interface PagesPanelProps {
  courseId: string;
  canEdit: boolean;
}

export function PagesPanel({ courseId, canEdit }: PagesPanelProps) {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const list = useInfiniteQuery(pagesQuery(courseId));
  const pages = list.data?.pages.flatMap((page) => page.items);
  return (
    <section className="flex min-w-0 flex-col gap-4" aria-labelledby={headingId}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Heading level={2} size="lg" id={headingId}>
          {t("authoring.pages.heading")}
        </Heading>
        {canEdit ? <NewPage courseId={courseId} /> : null}
      </div>
      <Text elementType="p" tone="sub">
        {t("authoring.pages.later")}
      </Text>
      {list.isError && pages === undefined ? (
        <LoadError error={list.error} level={3} onRetry={() => void list.refetch()} />
      ) : pages === undefined ? (
        <Skeleton className="h-16 w-full" />
      ) : pages.length === 0 ? (
        <Text elementType="p" tone="sub">
          {canEdit ? t("authoring.pages.empty.edit") : t("authoring.pages.empty")}
        </Text>
      ) : (
        <ul className="flex flex-col divide-y divide-line-soft border-y border-line-soft">
          {pages.map((page) => (
            <PageRow key={page.id} courseId={courseId} page={page} canEdit={canEdit} />
          ))}
        </ul>
      )}
      {list.hasNextPage ? (
        <Button
          className="self-start"
          isPending={list.isFetchingNextPage}
          onPress={() => void list.fetchNextPage()}
        >
          {t("authoring.pages.more")}
        </Button>
      ) : null}
    </section>
  );
}

function PageRow({
  courseId,
  page,
  canEdit,
}: {
  courseId: string;
  page: PageSummary;
  canEdit: boolean;
}) {
  const { t } = useStepclimbStrings();
  const router = useRouter();
  const href = router.buildLocation({
    to: "/c/$courseId/pages/$pageId/edit",
    params: { courseId, pageId: page.id },
  }).href;
  const details = t("authoring.pages.details", {
    number: page.current_version.number,
    updated: new Date(page.updated_at),
  });
  return (
    <li data-slot="page-row" className="flex min-w-0 flex-col gap-0.5 py-3">
      {canEdit ? (
        <Link href={href} className="wrap-anywhere self-start">
          {page.title}
        </Link>
      ) : (
        <Text tone="strong" className="wrap-anywhere">
          {page.title}
        </Text>
      )}
      <Text size="sm" tone="sub">
        {details}
      </Text>
    </li>
  );
}

function NewPage({ courseId }: { courseId: string }) {
  const { t } = useStepclimbStrings();
  return (
    <DialogTrigger>
      <Button intent="primary">{t("authoring.pages.new")}</Button>
      <Sheet title={t("authoring.pages.new.title")}>
        {({ close }) => <NewPageForm courseId={courseId} close={close} />}
      </Sheet>
    </DialogTrigger>
  );
}

function NewPageForm({ courseId, close }: { courseId: string; close: () => void }) {
  const { t } = useStepclimbStrings();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [title, setTitle] = useState("");
  const create = useMutation({
    mutationFn: (value: string) => createPage(courseId, value),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: pagesKey(courseId) }),
  });
  const problem = create.error ? authoringProblem(create.error, t) : undefined;
  // A taken or underived slug comes from the title, so the title field says it.
  const aboutTitle =
    problem?.field === "title" ||
    problem?.field === "slug" ||
    (create.error instanceof ApiError && create.error.problem?.code === "page.slug_taken");
  const fieldError = aboutTitle ? problem?.message : undefined;
  return (
    <Form
      aria-label={t("authoring.pages.new.title")}
      className="flex flex-col gap-5"
      onSubmit={(event) => {
        event.preventDefault();
        if (create.isPending) return;
        create.mutate(title, {
          onSuccess: (page) => {
            close();
            void navigate({
              to: "/c/$courseId/pages/$pageId/edit",
              params: { courseId, pageId: page.id },
            });
          },
        });
      }}
    >
      {problem !== undefined && fieldError === undefined ? (
        <FormError code={problem.code}>{problem.message}</FormError>
      ) : null}
      <TextField
        label={t("authoring.field.title")}
        description={t("authoring.pages.new.description")}
        value={title}
        onChange={(value) => {
          create.reset();
          setTitle(value);
        }}
        isRequired
        autoFocus
        isInvalid={fieldError !== undefined}
        errorMessage={fieldError}
      />
      <div className="flex flex-wrap justify-end gap-2">
        <Button onPress={close}>{t("authoring.cancel")}</Button>
        <Button type="submit" intent="primary" isPending={create.isPending}>
          {t("authoring.pages.new.submit")}
        </Button>
      </div>
    </Form>
  );
}
