// SPDX-License-Identifier: AGPL-3.0-or-later
// `/c/:course/m/:module/i/:item`, the deep link to an item (docs/specs/m1/03-modules-and-items.md
// B16). It reads `GET /api/v1/items/{item}` and opens the item only when the item's module and
// course are the ones the path names; otherwise, and on a 404 or 403, it shows the not-found
// view, the same whether the item does not exist, is unpublished, is in another organization or
// sits in another module. Signed out, the signed-in layout's 401 sends the browser to sign in
// with this address, query included, as the return target (B18). The item's content view
// (page, file, video, external link) is the item screen's (16-learning-ui-components.md B16);
// this view shows the item's title and, for a `file` item, its file (06-files.md B18).
import { Heading, Skeleton } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { useParams } from "@tanstack/react-router";
import { isAbsent, itemQuery, pathId } from "../course/outline";
import { FileItemView } from "../files/file-item-view";
import { LoadError } from "../layouts/load-error";
import { NotFound } from "./not-found";

export function CourseItemView() {
  const params = useParams({ strict: false });
  const courseId = pathId(params.courseId);
  const moduleId = pathId(params.moduleId);
  const itemId = pathId(params.itemId);
  if (courseId === undefined || moduleId === undefined || itemId === undefined) {
    return <NotFound level={2} />;
  }
  return <ItemLoader courseId={courseId} moduleId={moduleId} itemId={itemId} />;
}

interface ItemLoaderProps {
  courseId: string;
  moduleId: string;
  itemId: string;
}

function ItemLoader({ courseId, moduleId, itemId }: ItemLoaderProps) {
  const item = useQuery(itemQuery(itemId));
  if (item.isError) {
    if (isAbsent(item.error)) return <NotFound level={2} />;
    return (
      <div className="p-4 md:p-6">
        <LoadError error={item.error} level={2} onRetry={() => void item.refetch()} />
      </div>
    );
  }
  if (item.isPending) {
    return (
      <div className="p-4 md:p-6">
        <Skeleton className="h-8 w-64 max-w-full" />
      </div>
    );
  }
  if (item.data.course_id !== courseId || item.data.module_id !== moduleId) {
    return <NotFound level={2} />;
  }
  return (
    <article className="flex flex-col gap-4 p-4 md:p-6">
      <Heading level={2} size="xl" className="wrap-anywhere">
        {item.data.title}
      </Heading>
      <FileItemView item={item.data} level={3} />
    </article>
  );
}
