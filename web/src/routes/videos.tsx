// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/videos`, a course's video panel (docs/specs/m1/08-uploaded-video.md B1, B2, B22;
// 16-learning-ui-components.md B19): VideoUploader above the list of the course's videos with
// each one's status. The panel is for holders of `course.view_all`: the server refuses anyone
// else with 403, which the page says in words. The uploader shows only when the course's
// `permissions` hold `course.content_edit` and the course is not archived; the server checks every
// request regardless. While a video of the list is processing, the list is read again every 10 s,
// so a video uploaded before a reload, or by another editor, turns ready on screen.
import {
  Button,
  Code,
  ErrorState,
  Heading,
  POLL_MS,
  problemText,
  Skeleton,
  Text,
  useStepclimbStrings,
  VideoUploader,
} from "@stepclimb/ui";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { useId } from "react";
import { ApiError } from "../api";
import { type Course, courseQuery } from "../files/course-files";
import { LoadError } from "../layouts/load-error";
import { meQuery } from "../session";
import {
  courseVideosKey,
  courseVideosQuery,
  readVideoStatus,
  sendTus,
  type Video,
  videoUploadOwner,
  videoUploadsEndpoint,
} from "../videos/videos";

/** The key in `Course.permissions` the page reads (`02` B10). */
const CONTENT_EDIT = "course.content_edit";

/** What a refused or missing course shows instead of the videos, or undefined for another error. */
function refusalOf(error: unknown): "forbidden" | "notFound" | undefined {
  if (!(error instanceof ApiError)) return undefined;
  if (error.status === 403) return "forbidden";
  if (error.status === 404 && error.problem?.code === "course.not_found") return "notFound";
  return undefined;
}

function Refusal({ kind }: { kind: "forbidden" | "notFound" }) {
  const { t } = useStepclimbStrings();
  return kind === "forbidden" ? (
    <ErrorState title={t("videos.forbidden.title")} level={2}>
      {t("videos.forbidden.description")}
    </ErrorState>
  ) : (
    <ErrorState title={t("files.notFound.title")} level={2}>
      {t("files.notFound.description")}
    </ErrorState>
  );
}

export function VideosView() {
  const { courseId = "" } = useParams({ strict: false });
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
    <div className="flex max-w-3xl flex-col gap-6 p-4 md:p-6">
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
      <VideoPanel courseId={courseId} course={course.data} />
    </div>
  );
}

interface VideoPanelProps {
  courseId: string;
  course: Course | undefined;
  /**
   * The panel sits in a page that already says the course is archived (the course authoring
   * screen), so it leaves its own notice out.
   */
  embedded?: boolean;
}

/** The uploader and the list; the uploader stays mounted while the list loads and refreshes. */
export function VideoPanel({ courseId, course, embedded = false }: VideoPanelProps) {
  const { t } = useStepclimbStrings();
  const queryClient = useQueryClient();
  // The signed-in layout has loaded `/me`; the uploader waits for it, as its entries are the user's.
  const me = useQuery(meQuery).data;
  const owner = me === undefined ? undefined : videoUploadOwner(me);
  const archived = course?.status === "archived";
  const canEdit = (course?.permissions.includes(CONTENT_EDIT) ?? false) && !archived;
  const refresh = () => void queryClient.invalidateQueries({ queryKey: courseVideosKey(courseId) });
  return (
    <div className="flex min-w-0 flex-col gap-6">
      {archived && !embedded ? <Text elementType="p">{t("videos.archived")}</Text> : null}
      {canEdit && owner !== undefined ? (
        <VideoUploader
          key={owner}
          endpoint={videoUploadsEndpoint(courseId)}
          storageKey={courseId}
          owner={owner}
          send={sendTus}
          getVideo={readVideoStatus}
          onChange={refresh}
        />
      ) : null}
      <VideoList courseId={courseId} canEdit={canEdit} />
    </div>
  );
}

function isProcessing(pages: { items: Video[] }[] | undefined): boolean {
  return pages?.some((page) => page.items.some((v) => v.status === "processing")) ?? false;
}

function VideoList({ courseId, canEdit }: { courseId: string; canEdit: boolean }) {
  const { t } = useStepclimbStrings();
  const headingId = useId();
  const list = useInfiniteQuery({
    ...courseVideosQuery(courseId),
    refetchInterval: (query) => (isProcessing(query.state.data?.pages) ? POLL_MS : false),
  });
  const refusal = refusalOf(list.error);
  if (refusal !== undefined) return <Refusal kind={refusal} />;
  if (list.isError && list.data === undefined) {
    return <LoadError error={list.error} level={2} onRetry={() => void list.refetch()} />;
  }
  const videos = list.data?.pages.flatMap((page) => page.items);
  return (
    <section className="flex min-w-0 flex-col gap-3" aria-labelledby={headingId}>
      <Heading level={2} size="lg" id={headingId}>
        {t("videos.list")}
      </Heading>
      {videos === undefined ? (
        <Skeleton className="h-16 w-full" />
      ) : videos.length === 0 ? (
        <Text elementType="p" tone="sub">
          {canEdit ? t("videos.empty.edit") : t("videos.empty")}
        </Text>
      ) : (
        <ul className="flex flex-col divide-y divide-line-soft border-y border-line-soft">
          {videos.map((video) => (
            <VideoRow key={video.id} video={video} />
          ))}
        </ul>
      )}
      {list.hasNextPage ? (
        <Button
          className="self-start"
          isPending={list.isFetchingNextPage}
          onPress={() => void list.fetchNextPage()}
        >
          {t("videos.more")}
        </Button>
      ) : null}
    </section>
  );
}

function VideoRow({ video }: { video: Video }) {
  const { t } = useStepclimbStrings();
  const failure =
    video.status === "failed"
      ? problemText({ kind: "video", code: video.error_code ?? "video.failed" }, video.title, t)
      : undefined;
  return (
    <li data-slot="video-row" className="flex min-w-0 flex-col gap-0.5 py-3">
      <Text tone="strong" className="wrap-anywhere">
        {video.title}
      </Text>
      <Text size="sm" tone="sub">
        {t("videos.details", { status: video.status, created: new Date(video.created_at) })}
      </Text>
      {failure === undefined ? null : (
        <Text elementType="p" size="sm">
          {failure.message}
          {failure.code === undefined ? null : (
            <>
              {" "}
              <Code>{failure.code}</Code>
            </>
          )}
        </Text>
      )}
    </li>
  );
}
