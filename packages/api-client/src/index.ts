// SPDX-License-Identifier: Apache-2.0
// Stepclimb's API client: openapi-fetch typed by the generated schema.
import createClient, { type ClientOptions } from "openapi-fetch";
import type { components, paths } from "./schema";

export type { components, paths };
export type Health = components["schemas"]["Health"];

/** A client for the Stepclimb server at `baseUrl` ("" for the same origin as the page). */
export function createApiClient(options: ClientOptions = {}) {
  return createClient<paths>(options);
}

export type ApiClient = ReturnType<typeof createApiClient>;
