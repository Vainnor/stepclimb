# Install the pnpm workspace's packages from the lockfile
web-install:
    pnpm install --frozen-lockfile --reporter=silent

# Build the tokens, then the web app into web/dist (which the server embeds)
web-build: web-install
    pnpm --filter @stepclimb/tokens build
    pnpm --filter web build

# Biome: format and lint check of every TS, TSX, JSON and CSS file
web-lint: web-install
    pnpm biome check --error-on-warnings .

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
