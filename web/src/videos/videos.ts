// SPDX-License-Identifier: AGPL-3.0-or-later
// A course's videos through the public API (docs/specs/m1/08-uploaded-video.md B1, B2, B4, B22):
// the course's list a page at a time, one video's status for VideoUploader, and the uploader's
// tus requests, sent through `XMLHttpRequest` so a chunk reports its progress.
import type { components } from "@stepclimb/api-client";
import type { TusSend, VideoStatus } from "@stepclimb/ui";
import { infiniteQueryOptions } from "@tanstack/react-query";
import { ApiError, api, unwrap } from "../api";
import { sendWithProgress } from "../files/course-files";

export type Video = components["schemas"]["Video"];
type VideoPage = components["schemas"]["Page_Video"];

/** How many videos one list request asks for. */
const PAGE_LIMIT = 50;

/** Every query of one course's videos starts with this key, so one invalidation reaches them. */
export function courseVideosKey(courseId: string) {
  return ["courses", courseId, "videos"] as const;
}

/** The course's videos, oldest first, a page at a time. */
export function courseVideosQuery(courseId: string) {
  return infiniteQueryOptions({
    queryKey: courseVideosKey(courseId),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page: VideoPage) => page.next_cursor ?? undefined,
    queryFn: async ({ pageParam }): Promise<VideoPage> => {
      const query = {
        limit: PAGE_LIMIT,
        ...(pageParam === undefined ? {} : { cursor: pageParam }),
      };
      return unwrap(
        await api.GET("/api/v1/courses/{course_id}/videos", {
          params: { path: { course_id: courseId }, query },
        }),
      );
    },
  });
}

/** The tus creation address of the course (B4). */
export function videoUploadsEndpoint(courseId: string): string {
  const path = `/api/v1/courses/${encodeURIComponent(courseId)}/video-uploads`;
  return new URL(path, globalThis.location.origin).href;
}

/** VideoUploader's `getVideo`: the video's status, or `null` once it is gone (404). */
export async function readVideoStatus(
  id: string,
  signal: AbortSignal,
): Promise<VideoStatus | null> {
  try {
    const video = unwrap(
      await api.GET("/api/v1/videos/{video_id}", { params: { path: { video_id: id } }, signal }),
    );
    return { status: video.status, error_code: video.error_code ?? null };
  } catch (error) {
    if (error instanceof ApiError && error.problem?.code === "video.not_found") return null;
    throw error;
  }
}

/** VideoUploader's `send`: one tus request through `XMLHttpRequest`, with the body's progress. */
export const sendTus: TusSend = (request, body, onProgress, signal) =>
  sendWithProgress(request, body, onProgress, signal);
