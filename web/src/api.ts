// SPDX-License-Identifier: AGPL-3.0-or-later
// The one API client the app uses: the generated public client, on the page's own origin.
// `fetch` is looked up on each request rather than captured once, so tests can stub it.
import { type components, createApiClient } from "@stepclimb/api-client";

export const api = createApiClient({
  baseUrl: globalThis.location.origin,
  fetch: (request) => globalThis.fetch(request),
});

export type Problem = components["schemas"]["Problem"];

/**
 * An HTTP error answer from the server. `problem` is its problem+json body when it sent one. A
 * request that got no answer at all throws the `fetch` error instead, never an `ApiError`.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly problem: Problem | undefined;
  /** Seconds from a `Retry-After` header given in seconds (a 429), when the answer had one. */
  readonly retryAfter: number | undefined;

  constructor(status: number, problem: Problem | undefined, retryAfter?: number) {
    super(problem ? `${status} ${problem.code}` : `HTTP ${status}`);
    this.name = "ApiError";
    this.status = status;
    this.problem = problem;
    this.retryAfter = retryAfter;
  }
}

/** A `Retry-After` value in whole seconds; the HTTP-date form and anything else is undefined. */
export function retryAfterSeconds(header: string | null): number | undefined {
  if (header === null || !/^\d{1,9}$/.test(header.trim())) return undefined;
  return Number(header.trim());
}

function isProblem(body: unknown): body is Problem {
  return (
    typeof body === "object" &&
    body !== null &&
    "code" in body &&
    typeof body.code === "string" &&
    "title" in body &&
    typeof body.title === "string"
  );
}

/**
 * The data of an openapi-fetch result, or an `ApiError` thrown for its error answer, so TanStack
 * Query sees every failed request as an error.
 */
export function unwrap<T>(result: { data?: T; error?: unknown; response: Response }): T {
  if (!result.response.ok) {
    throw new ApiError(
      result.response.status,
      isProblem(result.error) ? result.error : undefined,
      retryAfterSeconds(result.response.headers.get("retry-after")),
    );
  }
  return result.data as T;
}

/** True for a 401: the request carried no session, or one that expired or was ended. */
export function isUnauthorized(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}
