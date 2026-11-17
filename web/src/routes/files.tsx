// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/files`, a course's files area (docs/specs/m1/16-learning-ui-components.md B20;
// 06-files.md B3, B9 to B12, Q5): the folder Tree beside the list of one folder's entries, with
// FileDrop upload into that folder and rename, move and delete on each row. The folder on screen
// is the `folder` search parameter, so a folder has an address. The area is for holders of
// `course.view_all` (`06` Q5): the server refuses anyone else with 403 `permission.denied`, which
// the page says in words. Upload and the row actions show only when the course's `permissions`
// hold `course.content_edit` and the course is not archived; the server checks every request
// regardless. Under 768 px the tree gives way to the breadcrumbs and the folder rows of the list.
import {
  Breadcrumb,
  Breadcrumbs,
  ErrorState,
  Heading,
  Skeleton,
  type Strings,
  Text,
  useStepclimbStrings,
} from "@stepclimb/ui";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useNavigate, useParams, useRouter, useSearch } from "@tanstack/react-router";
import { useState } from "react";
import { ApiError } from "../api";
import { type Course, courseQuery, folderPageQuery } from "../files/course-files";
import { FileTable } from "../files/file-table";
import { FolderTree } from "../files/folder-tree";
import { folderFromSearch, folderName, foldersDownTo, TOP_FOLDER } from "../files/folders";
import { UploadZone } from "../files/upload";
import { LoadError } from "../layouts/load-error";

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
  const { t } = useStepclimbStrings();
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
      <header className="flex min-w-0 flex-col gap-1">
        {course.data ? (
          <>
            <Heading level={2} size="xl" className="wrap-anywhere">
              {course.data.title}
            </Heading>
            <Text tone="sub">{course.data.code}</Text>
          </>
        ) : (
          <Skeleton className="h-8 w-64 max-w-full" />
        )}
      </header>
      {/* A folder's page cursors are its own: a new folder starts at its first page. */}
      <FolderArea key={folder} courseId={courseId} folder={folder} course={course.data} t={t} />
    </div>
  );
}

interface FolderAreaProps {
  courseId: string;
  folder: string;
  course: Course | undefined;
  t: Strings["t"];
}

function FolderArea({ courseId, folder, course, t }: FolderAreaProps) {
  const navigate = useNavigate();
  const folderHref = useFolderHref(courseId);
  // The cursors of the pages before this one; the last is this page's.
  const [cursors, setCursors] = useState<string[]>([]);
  const page = useQuery({
    ...folderPageQuery(courseId, folder, cursors.at(-1)),
    placeholderData: keepPreviousData,
  });
  const refusal = refusalOf(page.error);
  if (refusal !== undefined) return <Refusal kind={refusal} />;
  if (page.isError && page.data === undefined) {
    return <LoadError error={page.error} level={2} onRetry={() => void page.refetch()} />;
  }
  const archived = course?.status === "archived";
  const canEdit = (course?.permissions.includes(CONTENT_EDIT) ?? false) && !archived;
  const label = folderLabel(folder, t);
  const openFolder = (to: string) =>
    void navigate({
      to: "/c/$courseId/files",
      params: { courseId },
      search: to === TOP_FOLDER ? {} : { folder: to },
    });
  return (
    <div className="grid min-w-0 gap-6 md:grid-cols-[16rem_minmax(0,1fr)]">
      <FolderTree
        courseId={courseId}
        folder={folder}
        onOpenFolder={openFolder}
        className="hidden self-start md:flex"
      />
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
          entries={page.data?.items ?? []}
          isLoading={page.isPending || page.isPlaceholderData}
          canEdit={canEdit}
          folderHref={folderHref}
          pagination={
            cursors.length === 0 && !page.data?.next_cursor
              ? undefined
              : {
                  previousCursor: cursors.length > 0 ? (cursors.at(-2) ?? "") : null,
                  nextCursor: page.data?.next_cursor ?? null,
                  onPageChange: (cursor, direction) =>
                    setCursors((list) =>
                      direction === "next" ? [...list, cursor] : list.slice(0, -1),
                    ),
                }
          }
        />
      </div>
    </div>
  );
}
