// SPDX-License-Identifier: AGPL-3.0-or-later
// The one API client the app uses: the generated public client, on the page's own origin.
// `fetch` is looked up on each request rather than captured once, so tests can stub it.
import { createApiClient } from "@stepclimb/api-client";

export const api = createApiClient({
  baseUrl: globalThis.location.origin,
  fetch: (request) => globalThis.fetch(request),
});
