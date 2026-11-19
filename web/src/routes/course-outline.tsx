// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course` and `/c/:course/m/:module` (docs/specs/m1/03-modules-and-items.md B16): the
// course's modules with their items, and one module with its items, each item linking to its deep
// link. What the caller sees is the server's answer (B13, B14). A module whose course is not the
// one the path names, a 404 and a 403 show the not-found view, as for the item (B16). The course
// home's header, NextUp and ModuleRows are the course home screen's
// (16-learning-ui-components.md B16).
import { EmptyState, Heading, Link, Skeleton, useStepclimbStrings } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import {
  isAbsent,
  itemHref,
  type Module,
  moduleHref,
  moduleQuery,
  modulesQuery,
  pathId,
} from "../course/outline";
import { LoadError } from "../layouts/load-error";
import { NotFound } from "./not-found";

function Loading() {
  return (
    <div className="p-4 md:p-6">
      <Skeleton className="h-8 w-64 max-w-full" />
    </div>
  );
}

/** A module's items as links, in order; a header is a label, not a link. */
function ItemList({ module }: { module: Module }) {
  const { t } = useStepclimbStrings();
  if (module.items.length === 0) return <EmptyState>{t("course.module.empty")}</EmptyState>;
  return (
    <ul className="flex flex-col gap-2">
      {module.items.map((item) => (
        <li key={item.id} className="wrap-anywhere">
          {item.kind === "header" ? (
            <Heading level={3} size="base">
              {item.title}
            </Heading>
          ) : (
            <Link href={itemHref(item)}>{item.title}</Link>
          )}
        </li>
      ))}
    </ul>
  );
}

export function CourseHomeView() {
  const courseId = pathId(useParams({ strict: false }).courseId);
  if (courseId === undefined) return <NotFound level={2} />;
  return <CourseModules courseId={courseId} />;
}

function CourseModules({ courseId }: { courseId: string }) {
  const { t } = useStepclimbStrings();
  const modules = useQuery(modulesQuery(courseId));
  if (modules.isError) {
    if (isAbsent(modules.error)) return <NotFound level={2} />;
    return (
      <div className="p-4 md:p-6">
        <LoadError error={modules.error} level={2} onRetry={() => void modules.refetch()} />
      </div>
    );
  }
  if (modules.isPending) return <Loading />;
  if (modules.data.length === 0) return <EmptyState>{t("course.home.empty")}</EmptyState>;
  return (
    <div className="flex flex-col gap-8 p-4 md:p-6">
      {modules.data.map((module) => (
        <section key={module.id} className="flex flex-col gap-3">
          <Heading level={2} size="lg" className="wrap-anywhere">
            <Link href={moduleHref(module)}>{module.title}</Link>
          </Heading>
          <ItemList module={module} />
        </section>
      ))}
    </div>
  );
}

export function CourseModuleView() {
  const params = useParams({ strict: false });
  const courseId = pathId(params.courseId);
  const moduleId = pathId(params.moduleId);
  if (courseId === undefined || moduleId === undefined) return <NotFound level={2} />;
  return <ModuleLoader courseId={courseId} moduleId={moduleId} />;
}

function ModuleLoader({ courseId, moduleId }: { courseId: string; moduleId: string }) {
  const module = useQuery(moduleQuery(moduleId));
  if (module.isError) {
    if (isAbsent(module.error)) return <NotFound level={2} />;
    return (
      <div className="p-4 md:p-6">
        <LoadError error={module.error} level={2} onRetry={() => void module.refetch()} />
      </div>
    );
  }
  if (module.isPending) return <Loading />;
  if (module.data.course_id !== courseId) return <NotFound level={2} />;
  return (
    <section className="flex flex-col gap-4 p-4 md:p-6">
      <Heading level={2} size="xl" className="wrap-anywhere">
        {module.data.title}
      </Heading>
      <ItemList module={module.data} />
    </section>
  );
}
