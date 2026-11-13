#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Grep the files this branch touches for Stepclimb defect classes that grep can catch.
# Usage: .claude/scripts/pitfall-scan.sh [base-ref]   (default origin/next)
# Hits are leads, not verdicts: a hit can be pre-existing code in a changed file, or test code.
# Confirm each against the diff. Report-only: always exits 0 unless the base ref is missing.
# Adapted from the workflow-commands skill's scanner; add a check when a review finds a new class.
set -uo pipefail

base="${1:-origin/next}"
top="$(git rev-parse --show-toplevel)" || exit 2
cd "$top" || exit 2
git rev-parse --verify --quiet "$base" >/dev/null || { echo "base ref $base not found (git fetch origin?)" >&2; exit 2; }

hits=0
changed="$( { git diff --name-only --diff-filter=ACMR "$base"...HEAD; git diff --name-only --diff-filter=ACMR; git diff --cached --name-only --diff-filter=ACMR; git ls-files --others --exclude-standard; } | sort -u | grep -vE '(^|/)target/|\.lock$' )"

report() { # label, lines
  [[ -z "$2" ]] && return
  printf '\n%s\n' "$1"
  printf '%s\n' "$2" | sed 's/^/  /'
  hits=$((hits + $(printf '%s\n' "$2" | wc -l)))
}

# grep_changed <regex> <path-regex or ''> <label>
grep_changed() {
  local files
  files="$(printf '%s\n' "$changed" | grep -E "${2:-.}" || true)"
  [[ -z "$files" ]] && return
  report "$3" "$(printf '%s\n' "$files" | while IFS= read -r f; do [[ -f "$f" ]] && grep -HnE -- "$1" "$f"; done)"
}

# commits_matching <regex> <label>
commits_matching() {
  report "$2" "$(git log --format='%h %B' "$base"..HEAD | grep -E "$1" || true)"
}

lib='^crates/stepclimb-[a-z-]+/src/'
pure='^crates/stepclimb-(assess|activities|activities-atc)/src/'
ui='^(packages/ui|web/src|apps/ui-docs)/.*\.(tsx?|css)$'

grep_changed '^(<<<<<<<|>>>>>>>)( |$)|^=======$' '' "merge conflict marker"
commits_matching '^(Co-[Aa]uthored-[Bb]y:|🤖 Generated|Generated with \[)' "AI attribution in a commit message (AGENTS.md non-negotiable 1)"
report "env file added to git" "$(printf '%s\n' "$changed" | grep -E '(^|/)\.env(\..+)?$' | grep -v '\.example$' || true)"
report "local-only reference or private data in the change (content-and-ip.md)" \
  "$(printf '%s\n' "$changed" | grep -E '^(references|private)/' | grep -vE '^(references/(README|INDEX)|private/README)\.md$' || true)"
report "an applied migration was edited; add a new one (architecture.md -> Database)" \
  "$(git diff --name-only --diff-filter=MR "$base"...HEAD -- migrations/ || true)"
grep_changed '(sqlx::query(_as|_scalar)?|query(_as|_scalar)?!)\(' "$lib" "a query: does it filter by the OrgScope? (security-and-tenancy.md)"
grep_changed '\.route\(' '^crates/stepclimb-server/src/' "a new route: Require<P>, the four-case test and the cross-organization test (testing.md)"
grep_changed 'SystemTime::now|Instant::now|Utc::now|OffsetDateTime::now' "$lib" "wall-clock time in logic: take the clock as a parameter (testing.md -> Time)"
grep_changed 'thread_rng|rand::random|OsRng' "$pure" "unseeded randomness in grading or activities (assessment-integrity.md)"
grep_changed '\.(unwrap|expect)\(' "$lib" "unwrap/expect in a library: fine in tests, otherwise state the invariant (architecture.md)"
grep_changed '(dbg|println|eprintln)!\(' "$lib" "print or dbg in a library crate"
grep_changed '(info|warn|error|debug|trace)!\(.*(password|secret|token|api_key|cookie)' "$lib" "a secret named in a log macro: never log secrets (security-and-tenancy.md)"
grep_changed '#\[allow\(' '^crates/.*\.rs$' "lint suppression: needs a reason beside it"
grep_changed '#[0-9a-fA-F]{3,8}\b|rgba?\(|oklch\(|hsla?\(|-\[#' "$ui" "raw color outside packages/tokens (ui.md -> Tokens)"
grep_changed '\b(ml|mr|pl|pr|left|right)-[0-9[]' "$ui" "physical direction utility: use ms-/me-/ps-/pe-/start-/end- (ui.md)"
grep_changed 'asChild|@radix-ui|@headlessui' '\.(tsx?)$' "a second primitive layer or asChild (ui.md -> Components)"
report "emoji in JSX (ui.md)" "$(printf '%s\n' "$changed" | grep -E '\.tsx$' | while IFS= read -r f; do [[ -f "$f" ]] && grep -HnP '[\x{1F300}-\x{1FAFF}\x{2600}-\x{27BF}]' "$f"; done || true)"
grep_changed '(vatsim|vatusa|vzdc|artcc)' '^crates/stepclimb-(core|auth|assess|activities|interop|integrations|jobs|media)/' "VATSIM- or vZDC-specific code in the core (AGENTS.md non-negotiable)"
grep_changed '(Canvas|Moodle|instructure)' '^(crates|web|packages)/' "Canvas or Moodle named in code: UX reference only, never copied (content-and-ip.md)"

if [[ $hits -eq 0 ]]; then
  echo "pitfall-scan: no hits in $(printf '%s\n' "$changed" | grep -c . || true) changed file(s) against $base"
else
  printf '\npitfall-scan: %d hit(s) against %s. Confirm each against the diff.\n' "$hits" "$base"
fi
exit 0
