// SPDX-License-Identifier: AGPL-3.0-or-later
// A course's modules, items and pages through the public API, for the course authoring screen
// (docs/specs/m1/16-learning-ui-components.md B19; 03-modules-and-items.md B1 to B13;
// 04-unlock-and-completion-rules.md B1 to B3; 07-pages-and-the-editor.md B1). The server decides
// every refusal; these carry its answer.
import type { components } from "@stepclimb/api-client";
import type { BoardModule, MoveOutcome, Strings } from "@stepclimb/ui";
import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { ApiError, api, unwrap } from "../api";
import { authoringProblem } from "./problems";

export type Module = components["schemas"]["Module"];
export type Item = components["schemas"]["Item"];
export type ModulePatch = components["schemas"]["ModulePatch"];
export type ItemPatch = components["schemas"]["ItemPatch"];
export type NewItem = components["schemas"]["NewItem"];
export type PageSummary = components["schemas"]["PageSummary"];
type PageList = components["schemas"]["Page_PageSummary"];

/** The item kinds an author can create today (`03` B5): the others arrive with their specs. */
export const CREATABLE_KINDS = ["header", "external_link"] as const;
export type CreatableKind = (typeof CREATABLE_KINDS)[number];

export function modulesKey(courseId: string) {
  return ["courses", courseId, "modules"] as const;
}

export function pagesKey(courseId: string) {
  return ["courses", courseId, "pages"] as const;
}

/** The course's modules with every item, published or not: the caller holds `course.view_all`. */
export function modulesQuery(courseId: string) {
  return queryOptions({
    queryKey: modulesKey(courseId),
    queryFn: async (): Promise<Module[]> =>
      unwrap(
        await api.GET("/api/v1/courses/{course_id}/modules", {
          params: { path: { course_id: courseId } },
        }),
      ).modules,
  });
}

/** The modules as ModuleBoard draws them. */
export function boardModules(modules: readonly Module[]): BoardModule[] {
  return modules.map((m) => ({
    id: m.id,
    title: m.title,
    published: m.published,
    unlockAt: m.unlock_at === null || m.unlock_at === undefined ? null : new Date(m.unlock_at),
    prerequisiteIds: m.prerequisite_ids,
    requireSequential: m.require_sequential,
    items: m.items.map((i) => ({
      id: i.id,
      kind: i.kind,
      title: i.title,
      published: i.published,
      requirement: i.requirement ?? null,
    })),
  }));
}

/** A move's answer as ModuleBoard takes it: 409 `order.stale` asks for a reload. */
async function moveOutcome(write: () => Promise<unknown>, t: Strings["t"]): Promise<MoveOutcome> {
  try {
    await write();
    return { status: "moved" };
  } catch (error) {
    if (error instanceof ApiError && error.problem?.code === "order.stale") {
      return { status: "stale" };
    }
    return { status: "failed", message: authoringProblem(error, t).message };
  }
}

export function reorderModules(courseId: string, moduleIds: string[], t: Strings["t"]) {
  return moveOutcome(
    async () =>
      unwrap(
        await api.PUT("/api/v1/courses/{course_id}/modules/order", {
          params: { path: { course_id: courseId } },
          body: { module_ids: moduleIds },
        }),
      ),
    t,
  );
}

export function reorderItems(moduleId: string, itemIds: string[], t: Strings["t"]) {
  return moveOutcome(
    async () =>
      unwrap(
        await api.PUT("/api/v1/modules/{module_id}/items/order", {
          params: { path: { module_id: moduleId } },
          body: { item_ids: itemIds },
        }),
      ),
    t,
  );
}

export function moveItem(
  itemId: string,
  to: { moduleId: string; position: number },
  t: Strings["t"],
) {
  return moveOutcome(
    async () =>
      unwrap(
        await api.POST("/api/v1/items/{item_id}/move", {
          params: { path: { item_id: itemId } },
          body: { module_id: to.moduleId, position: to.position },
        }),
      ),
    t,
  );
}

export async function createModule(courseId: string, title: string): Promise<Module> {
  return unwrap(
    await api.POST("/api/v1/courses/{course_id}/modules", {
      params: { path: { course_id: courseId } },
      body: { title },
    }),
  );
}

export async function updateModule(moduleId: string, patch: ModulePatch): Promise<Module> {
  return unwrap(
    await api.PATCH("/api/v1/modules/{module_id}", {
      params: { path: { module_id: moduleId } },
      body: patch,
    }),
  );
}

export async function deleteModule(moduleId: string): Promise<void> {
  unwrap(
    await api.DELETE("/api/v1/modules/{module_id}", { params: { path: { module_id: moduleId } } }),
  );
}

export async function createItem(moduleId: string, item: NewItem): Promise<Item> {
  return unwrap(
    await api.POST("/api/v1/modules/{module_id}/items", {
      params: { path: { module_id: moduleId } },
      body: item,
    }),
  );
}

export async function updateItem(itemId: string, patch: ItemPatch): Promise<Item> {
  return unwrap(
    await api.PATCH("/api/v1/items/{item_id}", {
      params: { path: { item_id: itemId } },
      body: patch,
    }),
  );
}

export async function deleteItem(itemId: string): Promise<void> {
  unwrap(await api.DELETE("/api/v1/items/{item_id}", { params: { path: { item_id: itemId } } }));
}

/** How many pages one list request asks for. */
const PAGE_LIMIT = 50;

/** The course's pages by slug, a page of the list at a time. */
export function pagesQuery(courseId: string) {
  return infiniteQueryOptions({
    queryKey: pagesKey(courseId),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page: PageList) => page.next_cursor ?? undefined,
    queryFn: async ({ pageParam }): Promise<PageList> => {
      const query = {
        limit: PAGE_LIMIT,
        ...(pageParam === undefined ? {} : { cursor: pageParam }),
      };
      return unwrap(
        await api.GET("/api/v1/courses/{course_id}/pages", {
          params: { path: { course_id: courseId }, query },
        }),
      );
    },
  });
}

/** A new page with an empty body, its slug derived from the title by the server. */
export async function createPage(courseId: string, title: string): Promise<PageSummary> {
  return unwrap(
    await api.POST("/api/v1/courses/{course_id}/pages", {
      params: { path: { course_id: courseId } },
      body: { title },
    }),
  );
}
