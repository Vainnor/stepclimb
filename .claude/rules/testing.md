# Testing Rules (always loaded)

---

## Reading a result

- Read the summary line (nextest `Summary ... N passed`, Vitest `Tests N passed`, Playwright
  `N passed`), never the exit code alone. Zero tests run is not a pass for a change that should
  have added tests.
- Run the baseline before changing anything and classify any red as pre-existing or yours.
- Name the gates you did not run and why: "`just ci` green on x86_64 Linux; the macOS and Windows
  Tauri builds have not run" is a complete statement, "tests pass" is not.

## Layers (plan, Technical -> Testing)

| Layer | Where | What it proves |
| --- | --- | --- |
| Unit | `#[cfg(test)] mod tests` beside the code; `*.test.ts(x)` beside TS | One function: a config parse, a grading rule, a recipe |
| Property | `proptest` in `stepclimb-assess` and `-activities` | Renders are deterministic and never contain answers, scores stay in bounds, every generated problem passes `validate` and its solution passes `check`, across many seeds |
| Golden | per question type | A type's render and grade output for fixed inputs |
| Database | `#[sqlx::test]` in crate `tests/`, a fresh database per test | Queries, migrations, constraints |
| Permission | server `tests/` | Every protected route four ways: anonymous 401, missing permission 403, wrong scope 404, allowed 2xx |
| Cross-organization | server `tests/` | Every route refuses another organization's ids; a test walks the router so a new route cannot skip it |
| Contract | `just openapi-diff` | The committed OpenAPI document and the generated client match the code |
| Known cases | YAML per data set (M3) | Activity verdicts written with the TA; a failing case blocks publishing the data set |
| Web | Vitest + Testing Library | Component and route behavior |
| End to end | Playwright against the Compose stack (`just e2e`) | The built binary and SPA together; later mock-OIDC sign-in, a module, a timed quiz, essay grading |
| Accessibility | axe in Storybook and Playwright | Zero violations in every theme |
| Apps | Tauri build matrix in CI | Builds on macOS, Windows and Linux |
| Load | k6 before each minor release | The performance budgets in `docs/plan/technical.md` -> Hosting |

## Time and randomness

- Deadlines, expiry, cooldowns and reminders read a controllable clock passed in, never
  `SystemTime::now()` inside the logic, so tests can move time.
- Generated problems take a seed; a test names its seed and can be replayed.

## Writing tests

- Test behavior, not implementation: a test survives a refactor that keeps the behavior.
- A test must be able to fail. For a new guard, check or permission, break it once (a mutation)
  and watch the test go red before trusting it.
- Never hit the network outside the Compose stack. Identity providers, webhooks and connectors
  are tested against local mocks.
- Test data is synthetic. No real member names, emails, CIDs or exports (`content-and-ip.md`).

## Visual changes

Any change to what a component or page draws carries a screenshot or Storybook check in the PR:
which story or route, which themes (default and vZDC, light and dark), which width. An agent
cannot judge a render by imagining it; capture it and look.
