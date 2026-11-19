// SPDX-License-Identifier: AGPL-3.0-or-later
// The course layout (docs/specs/m1/03-modules-and-items.md B16, B17): the frame of the course
// home, a module and an item, inside the signed-in layout. Above the view it shows the "Back to
// <host>" link of the deep link's return URL when the organization lists that URL's origin, at
// every width.
import { Link, useStepclimbStrings } from "@stepclimb/ui";
import { useQuery } from "@tanstack/react-query";
import { Outlet, useSearch } from "@tanstack/react-router";
import { meQuery } from "../session";
import { useReturnLink } from "./return-link";

export interface CourseSearch {
  /** The deep link's return URL (B17), checked by `returnLink` before anything shows it. */
  return?: string;
}

/**
 * The course routes' search. A `return` that is not a string (the router reads `?return=1` as a
 * number) becomes the empty string, which makes no link and so clears the kept one, as any other
 * refused value does.
 */
export function validateCourseSearch(search: Record<string, unknown>): CourseSearch {
  if (search.return === undefined) return {};
  return { return: typeof search.return === "string" ? search.return : "" };
}

function BackLink() {
  const { t } = useStepclimbStrings();
  // The signed-in layout draws its routes only once `/me` has loaded.
  const origins = useQuery(meQuery).data?.embedding.return_origins ?? [];
  const fromAddress = useSearch({ strict: false, select: (search) => search.return });
  const link = useReturnLink(fromAddress, origins);
  if (link === undefined) return null;
  return (
    <nav aria-label={t("course.returnNav")} className="px-4 pt-4 md:px-6">
      <Link href={link.href}>{t("course.returnLink", { host: link.host })}</Link>
    </nav>
  );
}

export function CourseLayout() {
  return (
    <>
      <BackLink />
      <Outlet />
    </>
  );
}
