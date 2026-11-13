# Code Quality Rules (always loaded)

---

## Unjustified scope

Ship what was asked for and nothing else. A crate, module, endpoint, table, config flag,
component or dependency that no requirement calls for comes back out of the diff, however small
and however useful it might one day be. Name the spec section or issue every addition serves.
"It might be needed later" is not a requirement. A fix to a defect your own change introduced,
and work the user approved this session, are in scope.

## Errors

- Library crates return typed errors (`thiserror`); never `panic!` on bad input (a malformed
  import, an unknown question type, a bad token). Panics are for violated internal invariants,
  stated in a comment beside them.
- API errors are `application/problem+json` (RFC 9457) with a stable `code`; never leak an
  internal error message or a stack to the client.
- No silent fallbacks. A missing value in config or a data set is an error naming the field, not
  a default that hides it. If a default is intended, the format documents it.
- Never swallow an error to keep a request or job going. Log it once with context and surface it.
- TypeScript: no `any`, no non-null `!` on data from the network; parse and narrow it.

## Complexity

- Functions under about 40 lines; nesting under 3 levels.
- No god modules or god components. Split by concept.
- Prefer plain data and functions over traits until a second implementation exists. The plan's
  `QuestionType` and `Activity` traits are the stated exceptions.

## Comments

- Describe the code as it is now. No issue numbers, dates, "fixed", "previously" or debugging
  stories in source; that history belongs in the commit and the test.
- Say why, not what.

## Verify before writing the mechanism down

A comment, doc or commit that states how something works must be checked against the code or a
run first. Unchecked, write "evidence points at".

## Sibling sweeps

When you fix a pattern in one place (a query missing its `org_id`, a route missing its permission
extractor, a raw color in a component), grep for the same pattern across the workspace and fix or
list every instance in the same change.

## Dead code

Before deleting, ask: is it called? Is it reached without a direct call (serde, a route
registered with the router, a job handler, a Storybook story)? The placeholder crates and
packages are not dead code; their milestones are in `docs/plan/technical.md` -> Milestones.
