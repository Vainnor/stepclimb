// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/edit`, the course authoring screen (docs/specs/m1/16-learning-ui-components.md B19,
// B13; 03-modules-and-items.md; 04-unlock-and-completion-rules.md B1 to B3; 07 B1; 08 D36). Three
// panels, the open one in the `panel` search parameter: the modules on ModuleBoard, with adding,
// editing, publishing, deleting and moving modules and items; the course's pages, each opening in
// the page editor; and the video panel. The files area is linked. The screen is for holders of
// `course.view_all`; the server refuses anyone else's writes regardless. Editing shows only when
// the course's `permissions` hold `course.content_edit` and the course is not archived; otherwise
// the board is read-only and the screen says why.
import {
  ButtonLink,
  ErrorState,
  Heading,
  Skeleton,
  Tab,
  TabList,
  TabPanel,
  Tabs,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useParams, useRouter, useSearch } from "@tanstack/react-router";
import { ApiError } from "../api";
import { ModulesPanel } from "../authoring/modules-panel";
import { PagesPanel } from "../authoring/pages-panel";
import { type Course, courseQuery } from "../files/course-files";
import { LoadError } from "../layouts/load-error";
import { VideoPanel } from "./videos";

/** The keys in `Course.permissions` the screen reads (`02` B10). */
const VIEW_ALL = "course.view_all";
const CONTENT_EDIT = "course.content_edit";

export const PANELS = ["modules", "pages", "videos"] as const;
export type Panel = (typeof PANELS)[number];

export interface CourseEditSearch {
  /** The open panel; the modules when absent. */
  panel?: Panel;
}

export function validateCourseEditSearch(search: Record<string, unknown>): CourseEditSearch {
  const panel = PANELS.find((p) => p === search.panel);
  return panel === undefined || panel === "modules" ? {} : { panel };
}

type Refusal = "forbidden" | "notFound";

/** What a refused or missing course shows instead of the screen, or undefined for another error. */
function refusalOf(error: unknown): Refusal | undefined {
  if (!(error instanceof ApiError)) return undefined;
  if (error.status === 403) return "forbidden";
  if (error.status === 404 && error.problem?.code === "course.not_found") return "notFound";
  return undefined;
}

function RefusalState({ kind }: { kind: Refusal }) {
  const { t } = useStepclimbStrings();
  return kind === "forbidden" ? (
    <ErrorState title={t("authoring.forbidden.title")} level={2}>
      {t("authoring.forbidden.description")}
    </ErrorState>
  ) : (
    <ErrorState title={t("files.notFound.title")} level={2}>
      {t("files.notFound.description")}
    </ErrorState>
  );
}

export function CourseEditView() {
  const { courseId = "" } = useParams({ strict: false });
  const course = useQuery(courseQuery(courseId));
  if (course.isError) {
    const refusal = refusalOf(course.error);
    return (
      <div className="p-4 md:p-6">
        {refusal === undefined ? (
          <LoadError error={course.error} level={2} onRetry={() => void course.refetch()} />
        ) : (
          <RefusalState kind={refusal} />
        )}
      </div>
    );
  }
  if (course.data !== undefined && !course.data.permissions.includes(VIEW_ALL)) {
    return (
      <div className="p-4 md:p-6">
        <RefusalState kind="forbidden" />
      </div>
    );
  }
  return (
    <div className="flex min-w-0 flex-col gap-6 p-4 md:p-6">
      <CourseTitle courseId={courseId} course={course.data} />
      {course.data === undefined ? (
        <Skeleton className="h-64 w-full" />
      ) : (
        <Panels courseId={courseId} course={course.data} />
      )}
    </div>
  );
}

function CourseTitle({ courseId, course }: { courseId: string; course: Course | undefined }) {
  const { t } = useStepclimbStrings();
  const router = useRouter();
  const filesHref = router.buildLocation({ to: "/c/$courseId/files", params: { courseId } }).href;
  if (course === undefined) return <Skeleton className="h-8 w-64 max-w-full" />;
  return (
    <header className="flex min-w-0 flex-col gap-1">
      <Heading level={2} size="xl" className="wrap-anywhere">
        {course.title}
      </Heading>
      <Text tone="sub">
        {t("authoring.courseLine", { code: course.code, status: course.status })}
      </Text>
      <ButtonLink href={filesHref} className="mt-2 self-start">
        {t("authoring.files")}
      </ButtonLink>
    </header>
  );
}

/** Why the content cannot be changed here, or undefined when it can. */
function readOnlyReason(course: Course): "archived" | "readOnly" | undefined {
  if (course.status === "archived") return "archived";
  if (!course.permissions.includes(CONTENT_EDIT)) return "readOnly";
  return undefined;
}

function Panels({ courseId, course }: { courseId: string; course: Course }) {
  const { t } = useStepclimbStrings();
  const navigate = useNavigate();
  const panel = useSearch({ strict: false }).panel ?? "modules";
  const reason = readOnlyReason(course);
  const canEdit = reason === undefined;
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {reason === undefined ? null : (
        <Text elementType="p" data-slot="authoring-read-only">
          {reason === "archived" ? t("authoring.archived") : t("authoring.readOnly")}
        </Text>
      )}
      <Tabs
        selectedKey={panel}
        onSelectionChange={(key) =>
          void navigate({
            to: "/c/$courseId/edit",
            params: { courseId },
            search: key === "modules" ? {} : { panel: key as Panel },
            replace: true,
          })
        }
      >
        <TabList aria-label={t("authoring.panels")}>
          <Tab id="modules">{t("authoring.panel.modules")}</Tab>
          <Tab id="pages">{t("authoring.panel.pages")}</Tab>
          <Tab id="videos">{t("authoring.panel.videos")}</Tab>
        </TabList>
        <TabPanel id="modules">
          <ModulesPanel courseId={courseId} canEdit={canEdit} />
        </TabPanel>
        <TabPanel id="pages">
          <PagesPanel courseId={courseId} canEdit={canEdit} />
        </TabPanel>
        <TabPanel id="videos">
          <VideoPanel courseId={courseId} course={course} embedded />
        </TabPanel>
      </Tabs>
    </div>
  );
}
