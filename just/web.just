# Install the pnpm workspace's packages from the lockfile
web-install:
    pnpm install --frozen-lockfile --reporter=silent

# Build the tokens, then the web app into web/dist (which the server embeds)
web-build: web-install
    pnpm --filter @stepclimb/tokens build
    pnpm --filter web build

# Biome: format and lint check of every TS, TSX, JSON and CSS file, with Stepclimb UI's icon and
# className rules; then the UI pattern lint (raw colors, palette, physical directions, emoji) and
# its fixture tests, which compare the className rule with the token build
web-lint: web-install changeset-check
    pnpm biome check --error-on-warnings .
    pnpm --filter @stepclimb/tokens build
    node --test bin/check-ui-lint.test.mjs
    node bin/check-ui-lint.mjs

# Fails when a versioned package (packages/ui, packages/tokens) changed since origin/next and no
# changeset on the branch names it (.changeset/README.md)
changeset-check: web-install
    node --test bin/check-changesets.test.mjs
    node bin/check-changesets.mjs origin/next

# Biome: apply formatting and safe fixes
web-fmt: web-install
    pnpm biome check --write .

# tsc over every package
typecheck: web-install
    pnpm -r --no-bail typecheck

# Vitest in every package that has tests (the tokens test reads the build)
test-web: web-build
    pnpm -r --no-bail test

# The Vite dev server on :5173, proxying the API to `just serve`
dev: web-install
    pnpm --filter web dev

# Storybook on :6006
storybook: web-build
    pnpm --filter @stepclimb/ui-docs dev

# The Tauri shell for this machine's OS: release build, unbundled and unsigned, after a
# cargo-deny check of its own workspace. Not part of `just ci`; required for changes to apps/app
# (.claude/rules/cross-platform.md). macOS and Windows builds run on those machines.
app-build: web-build
    cargo deny --manifest-path apps/app/src-tauri/Cargo.toml check
    pnpm --filter @stepclimb/app build

# Builds Storybook, then runs the story suite in Playwright's pinned image: a screenshot against
# apps/ui-docs/visual/baselines and axe for every story, theme and width (bin/ui-visual.sh).
# `just ui-visual --update-snapshots` rewrites the baselines after an intended change
ui-visual *args: web-install
    pnpm --filter @stepclimb/tokens build
    pnpm --filter @stepclimb/ui-docs build
    bash bin/ui-visual.sh {{ args }}
