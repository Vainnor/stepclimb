// SPDX-License-Identifier: AGPL-3.0-or-later
// A course's outline through the public API (docs/specs/m1/03-modules-and-items.md B13 to B16):
// the course's modules with their items, one module, one item. The server decides what the
// caller sees; an unpublished, draft or other organization's module or item answers 404.
import type { components } from "@stepclimb/api-client";
import { queryOptions } from "@tanstack/react-query";
import { ApiError, api, unwrap } from "../api";

export type Module = components["schemas"]["Module"];
export type Item = components["schemas"]["Item"];

export function modulesQuery(courseId: string) {
  return queryOptions({
    queryKey: ["courses", courseId, "modules"],
    queryFn: async (): Promise<Module[]> =>
      unwrap(
        await api.GET("/api/v1/courses/{course_id}/modules", {
          params: { path: { course_id: courseId } },
        }),
      ).modules,
  });
}

export function moduleQuery(moduleId: string) {
  return queryOptions({
    queryKey: ["modules", moduleId],
    queryFn: async (): Promise<Module> =>
      unwrap(
        await api.GET("/api/v1/modules/{module_id}", { params: { path: { module_id: moduleId } } }),
      ),
  });
}

export function itemQuery(itemId: string) {
  return queryOptions({
    queryKey: ["items", itemId],
    queryFn: async (): Promise<Item> =>
      unwrap(await api.GET("/api/v1/items/{item_id}", { params: { path: { item_id: itemId } } })),
  });
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * The id a path segment names, in the lowercase form the server answers with, or undefined when
 * the segment is no UUID: such a path names nothing, and no request is sent for it.
 */
export function pathId(segment: string | undefined): string | undefined {
  return segment !== undefined && UUID.test(segment) ? segment.toLowerCase() : undefined;
}

/**
 * True when a read's error means the path names nothing the caller may see: 404, and 403 too, so
 * a deep link answers the same whether an item does not exist, is unpublished, is in another
 * organization or sits in a course the caller holds no `course.view` at (B16).
 */
export function isAbsent(error: unknown): boolean {
  return error instanceof ApiError && (error.status === 404 || error.status === 403);
}

/** The deep link of `item` (B16). */
export function itemHref(item: Pick<Item, "course_id" | "module_id" | "id">): string {
  return `/c/${item.course_id}/m/${item.module_id}/i/${item.id}`;
}

/** The address of `module` in its course. */
export function moduleHref(module: Pick<Module, "course_id" | "id">): string {
  return `/c/${module.course_id}/m/${module.id}`;
}
