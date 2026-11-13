# @stepclimb/api-client

The TypeScript client for Stepclimb's public API, generated from the OpenAPI document the server
publishes (`openapi/stepclimb.json`, served at `/api/openapi.json`). Every Stepclimb client,
the web app included, talks to the server through this package; there are no private endpoints.

Licensed under the Apache License 2.0 (`LICENSE`), unlike the rest of the repository, which is
AGPL-3.0-or-later. The plan licenses SDK packages permissively so other systems can embed them.

- `src/schema.ts` is generated: `pnpm --filter @stepclimb/api-client generate` (or
  `just openapi`). `just openapi-diff` fails when it is out of date.
- `src/index.ts` wraps [`openapi-fetch`](https://openapi-ts.dev/openapi-fetch/) with the
  generated types.
