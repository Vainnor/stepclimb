// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/files`, a course's files area (docs/specs/m1/16-learning-ui-components.md B20,
// B20a; 06-files.md B3, B9 to B12, Q5): the folder Tree beside the list of one folder's entries,
// with FileDrop upload into that folder and rename, move and delete on each row. The folder on
// screen is the `folder` search parameter, so a folder has an address; one the server refuses as
// a path says so and links to the top folder. The area is for holders of `course.view_all`
// (`06` Q5): the server refuses anyone else with 403 `permission.denied`, which the page says in
// words. Upload and the row actions show only when the course's `permissions` hold
// `course.content_edit` and the course is not archived; the server checks every request
// regardless. The tree is mounted from 768 px; under that the breadcrumbs and the folder rows of
// the list lead between folders.
import {
  Breadcrumb,
  Breadcrumbs,
  ButtonLink,
  ErrorState,
  Heading,
  MENU_POPOVER_QUERY,
  type PaginationProps,
  Skeleton,
  type Strings,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useNavigate, useParams, useRouter, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { ApiError } from "../api";
import { type Course, courseQuery, type FolderEntry, folderPageQuery } from "../files/course-files";
import { FileTable } from "../files/file-table";
import { FolderTree } from "../files/folder-tree";
import { folderFromSearch, folderName, foldersDownTo, TOP_FOLDER } from "../files/folders";
import { isInvalidFolder } from "../files/problems";
import { UploadZone } from "../files/upload";
import { LoadError } from "../layouts/load-error";
import { useMediaQuery } from "../use-media-query";

/** The keys in `Course.permissions` the page reads (`02` B10). */
const CONTENT_EDIT = "course.content_edit";

/** What a refused or missing course shows instead of the files, or undefined for another error. */
function refusalOf(error: unknown): "forbidden" | "notFound" | undefined {
  if (!(error instanceof ApiError)) return undefined;
  if (error.status === 403) return "forbidden";
  if (error.status === 404 && error.problem?.code === "course.not_found") return "notFound";
  return undefined;
}

function Refusal({ kind }: { kind: "forbidden" | "notFound" }) {
  const { t } = useStepclimbStrings();
  return kind === "forbidden" ? (
    <ErrorState title={t("files.forbidden.title")} level={2}>
      {t("files.forbidden.description")}
    </ErrorState>
  ) : (
    <ErrorState title={t("files.notFound.title")} level={2}>
      {t("files.notFound.description")}
    </ErrorState>
  );
}

/** A `folder` the server refused as a path: no retry can help, the top folder can. */
function InvalidFolder({ topHref }: { topHref: string }) {
  const { t } = useStepclimbStrings();
  return (
    <ErrorState
      title={t("files.badFolder.title")}
      level={2}
      action={<ButtonLink href={topHref}>{t("files.badFolder.top")}</ButtonLink>}
    >
      {t("files.badFolder.description")}
    </ErrorState>
  );
}

function folderLabel(folder: string, t: Strings["t"]): string {
  return folderName(folder) ?? t("files.topFolder");
}

/** The files area's address at `folder`. */
function useFolderHref(courseId: string): (folder: string) => string {
  const router = useRouter();
  return (folder) =>
    router.buildLocation({
      to: "/c/$courseId/files",
      params: { courseId },
      search: folder === TOP_FOLDER ? {} : { folder },
    }).href;
}

export function FilesView() {
  const { courseId = "" } = useParams({ strict: false });
  const folder = folderFromSearch(useSearch({ strict: false }).folder);
  const course = useQuery(courseQuery(courseId));
  if (course.isError) {
    const refusal = refusalOf(course.error);
    return (
      <div className="p-4 md:p-6">
        {refusal === undefined ? (
          <LoadError error={course.error} level={2} onRetry={() => void course.refetch()} />
        ) : (
          <Refusal kind={refusal} />
        )}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-6 p-4 md:p-6">
      <CourseTitle course={course.data} />
      {/* A folder's page cursors are its own: a new folder starts at its first page. */}
      <FolderArea key={folder} courseId={courseId} folder={folder} course={course.data} />
    </div>
  );
}

function CourseTitle({ course }: { course: Course | undefined }) {
  return (
    <header className="flex min-w-0 flex-col gap-1">
      {course ? (
        <>
          <Heading level={2} size="xl" className="wrap-anywhere">
            {course.title}
          </Heading>
          <Text tone="sub">{course.code}</Text>
        </>
      ) : (
        <Skeleton className="h-8 w-64 max-w-full" />
      )}
    </header>
  );
}

/**
 * The API pages a folder with `next_cursor` only. The first page has no cursor: it is the
 * previous page's cursor this table hands Pagination for the second page, and asks for without
 * one.
 */
const FIRST_PAGE = "";

type PaginationOptions = Omit<PaginationProps, "className">;

/**
 * Previous and Next through a folder, from the cursors of the pages visited so far: the cursor
 * of the page on screen, and the Pagination options once its `next_cursor` is known.
 */
function useCursorTrail() {
  // The cursors of the pages after the first that led here; the last is this page's.
  const [trail, setTrail] = useState<string[]>([]);
  const current = trail.at(-1);
  const pagination = (nextCursor: string | null | undefined): PaginationOptions | undefined =>
    trail.length === 0 && !nextCursor
      ? undefined
      : {
          previousCursor: trail.length === 0 ? null : (trail.at(-2) ?? FIRST_PAGE),
          nextCursor: nextCursor ?? null,
          onPageChange: (cursor, direction) =>
            setTrail((visited) =>
              direction === "next" ? [...visited, cursor] : visited.slice(0, -1),
            ),
        };
  return { cursor: current === FIRST_PAGE ? undefined : current, pagination };
}

interface FolderAreaProps {
  courseId: string;
  folder: string;
  course: Course | undefined;
}

function FolderArea({ courseId, folder, course }: FolderAreaProps) {
  const folderHref = useFolderHref(courseId);
  const trail = useCursorTrail();
  const page = useQuery({
    ...folderPageQuery(courseId, folder, trail.cursor),
    placeholderData: keepPreviousData,
  });
  const refusal = refusalOf(page.error);
  if (refusal !== undefined) return <Refusal kind={refusal} />;
  if (isInvalidFolder(page.error)) return <InvalidFolder topHref={folderHref(TOP_FOLDER)} />;
  if (page.isError && page.data === undefined) {
    return <LoadError error={page.error} level={2} onRetry={() => void page.refetch()} />;
  }
  return (
    <div className="grid min-w-0 gap-6 md:grid-cols-[16rem_minmax(0,1fr)]">
      <WideTree courseId={courseId} folder={folder} />
      <FolderList
        courseId={courseId}
        folder={folder}
        course={course}
        entries={page.data?.items}
        isLoading={page.isPending || page.isPlaceholderData}
        pagination={trail.pagination(page.data?.next_cursor)}
      />
    </div>
  );
}

/** The folder tree, mounted only where it shows: at 768 px and wider. */
function WideTree({ courseId, folder }: { courseId: string; folder: string }) {
  const navigate = useNavigate();
  const wide = useMediaQuery(MENU_POPOVER_QUERY);
  if (!wide) return null;
  return (
    <FolderTree
      courseId={courseId}
      folder={folder}
      onOpenFolder={(to) =>
        void navigate({
          to: "/c/$courseId/files",
          params: { courseId },
          search: to === TOP_FOLDER ? {} : { folder: to },
        })
      }
      className="self-start"
    />
  );
}

interface FolderListProps {
  courseId: string;
  folder: string;
  course: Course | undefined;
  entries: FolderEntry[] | undefined;
  isLoading: boolean;
  pagination: PaginationOptions | undefined;
}

function FolderList({ courseId, folder, course, entries, isLoading, pagination }: FolderListProps) {
  const { t } = useStepclimbStrings();
  const folderHref = useFolderHref(courseId);
  const archived = course?.status === "archived";
  const canEdit = (course?.permissions.includes(CONTENT_EDIT) ?? false) && !archived;
  const label = folderLabel(folder, t);
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Breadcrumbs>
        {foldersDownTo(folder).map((path) => (
          <Breadcrumb key={path} id={path} href={folderHref(path)}>
            {folderLabel(path, t)}
          </Breadcrumb>
        ))}
      </Breadcrumbs>
      {archived ? <Text elementType="p">{t("files.archived")}</Text> : null}
      {canEdit ? <UploadZone courseId={courseId} folder={folder} folderLabel={label} /> : null}
      <FileTable
        courseId={courseId}
        folder={folder}
        label={t("files.list", { folder: label })}
        entries={entries ?? []}
        isLoading={isLoading}
        canEdit={canEdit}
        folderHref={folderHref}
        pagination={pagination}
      />
    </div>
  );
}
