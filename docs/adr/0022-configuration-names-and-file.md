# 0022. Configuration names and the stepclimb.toml layout

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decider:** the owner, accepting the recommendation of `docs/specs/m0/14-compose-deploy.md` Q2
  on 2026-11-13
- **Related:** `docs/plan/technical.md` -> Hosting and operations (Configuration);
  `docs/specs/m0/14-compose-deploy.md` B8 to B11, Q2; `docs/specs/m0/03-users-and-identities.md`
  Q5 (the naming half); `docs/specs/m0/08-sessions-and-csrf.md` (`STEPCLIMB_PUBLIC_URL`)

## Context

The plan puts `STEPCLIMB_*` environment variables over an optional `stepclimb.toml`, with
`*_FILE` variants for secrets, and stops startup on bad configuration. It does not say how a
variable maps to a TOML key, where the file is found, or what an unrecognized variable does.
Before this decision the server read four flat variables: `STEPCLIMB_BIND`,
`STEPCLIMB_DATABASE_URL(_FILE)`, `STEPCLIMB_ENCRYPTION_KEYS(_FILE)` and `STEPCLIMB_LOG`. Spec 14
Q2, the naming half of spec 03 Q5 and spec 08's public-URL question asked the same thing; this
record answers the naming for all three. How `[[providers]]` in the file relates to the
`identity_providers` table (spec 03 Q5's other half) is decided in spec 03 and is not recorded
here.

## Options

### Option A: sections joined by `__`
`[database] url` maps to `STEPCLIMB_DATABASE__URL`. Groups related keys, but renames every
existing variable and the double underscore is easy to mistype.

### Option B: flat keys matching flat variables
`database_url` maps to `STEPCLIMB_DATABASE_URL`. Keeps every existing name. Arrays of tables such
as the plan's `[[providers]]` exist in the file only.

### Option C: sections with single `_` variables through a fixed table
Readable in both places, but the mapping lives in a table that every new key must update.

## Recommendation

Option B, with the file named by `--config` or `STEPCLIMB_CONFIG` and no default path, and an
unknown `STEPCLIMB_*` variable refused.

## Decision

Option B. Each setting has one variable, `STEPCLIMB_` and the key in upper case, and one
top-level TOML key; a secret also has `{VAR}_FILE` and `{key}_file`. A variable overrides the
file setting by setting. The file is read only when `--config` or `STEPCLIMB_CONFIG` names it;
there is no default path. An unknown `STEPCLIMB_*` variable and an unknown TOML key stop startup,
because a misspelled secret's name would otherwise start the server without it.

## Consequences

New settings keep the flat shape (`STEPCLIMB_PUBLIC_URL` and `public_url`). Settings that are
lists of records, such as `[[providers]]`, are TOML only and have no variable.

The `STEPCLIMB_` prefix belongs to the server. Development tooling that reads its own variables
uses the `SC_DEV_` prefix (`SC_DEV_PG_PORT`, `SC_DEV_PORT`, `SC_DEV_MINIO_PORT`,
`SC_DEV_MINIO_CONSOLE_PORT` in the development Compose file, `SC_DEV_SERVER` for Vite) or `SC_`
(`SC_E2E_URL` for Playwright), so a developer's shell never holds a `STEPCLIMB_*` variable the
server would refuse.
