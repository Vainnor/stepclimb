# 0022. Configuration names and the stepclimb.toml layout

- **Status:** Accepted
- **Date:** 2026-11-13
- **Decider:** the owner, accepting the recommendation of `docs/specs/m0/14-compose-deploy.md` Q2
  on 2026-11-13
- **Related:** `docs/plan/technical.md` -> Hosting and operations (Configuration);
  `docs/specs/m0/14-compose-deploy.md` B8 to B11, Q2; `docs/specs/m0/03-users-and-identities.md`
  Q5 (names); `docs/specs/m0/08-sessions-and-csrf.md` (`STEPCLIMB_PUBLIC_URL`)

## Context

The plan puts `STEPCLIMB_*` environment variables over an optional `stepclimb.toml`, with
`*_FILE` variants for secrets, and stops startup on bad configuration. It does not say how a
variable maps to a TOML key, where the file is found, or what an unrecognized variable does.
Before this decision the server read four flat variables: `STEPCLIMB_BIND`,
`STEPCLIMB_DATABASE_URL(_FILE)`, `STEPCLIMB_ENCRYPTION_KEYS(_FILE)` and `STEPCLIMB_LOG`. Spec 14
Q2, spec 03 Q5 and spec 08's public-URL question asked the same thing; this record answers all
three.

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
lists of records, such as `[[providers]]`, are TOML only and have no variable. Any other program
reading a `STEPCLIMB_*` variable from the same environment (the Compose file's port variables,
`STEPCLIMB_DEV_SERVER` for Vite) must not export it into the server's environment, or the server
refuses to start.
