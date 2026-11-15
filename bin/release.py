# SPDX-License-Identifier: AGPL-3.0-or-later
"""The release recipe (docs/specs/m0/01-repo-and-ci.md B15; docs/adr/0030-release-merge-commits.md).

`notes <x.y.z>` (run by `just release-version`) adds the version's section to CHANGELOG.md:
git-cliff over the commits after the previous release tag up to HEAD. `prepare <x.y.z>`
(`just release-prepare`) branches from origin/next, runs `just release-version`, commits the
version and the notes (whose commit body names the commit the notes end at), writes the release
PR's body to target/release/ and prints the commands a person runs next. `verify <vX.Y.Z>` and
`finish <vX.Y.Z>` run before and after `just ci` in `just release`, in a clean worktree of the
tag: they check the tag, regenerate the notes and compare them with CHANGELOG.md, write the notes
file and check the `just ci` pass. bin/release_artifacts.py then builds the image and the SBOMs,
and its `publish` is the one step that publishes.

Nothing here pushes, tags or publishes; it fetches from origin and works on the local checkout.
`prepare` makes every check before its first change, so a refusal leaves the checkout as it was.
cliff.toml, the release PR template and the attribution patterns are read from the checkout
being released.

Usage: release.py notes <x.y.z> | release.py prepare <x.y.z> <just>
     | release.py verify <vX.Y.Z> | release.py finish <vX.Y.Z>"""

from __future__ import annotations

import difflib
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from string import Template

sys.path.insert(0, str(Path(__file__).resolve().parent))

import versions  # noqa: E402

REMOTE = "origin"
CHANGELOG = "CHANGELOG.md"
CLIFF = "cliff.toml"
PR_TEMPLATE = ".github/release-pr-template.md"
OUT = Path("target/release")
ATTRIBUTION = ".claude/hooks/lib/attribution.sh"
RELEASE_SUBJECT = "chore(release)"
BASE_PREFIX = "The v{version} notes cover origin/next up to "
CHANGELOG_HEADER = """# Changelog

Stepclimb's releases, newest first. `just release-version` writes each section with git-cliff
from the Conventional Commit subjects merged into `next` since the previous release, and
`just release` refuses a tag whose section differs from what git-cliff writes. One version covers
the server, the web app and the Tauri app. Before 1.0 the version is `0.<milestone>.<patch>`.
"""


class ReleaseError(Exception):
    """A check failed or a command could not run; the message says which and what to do."""


def git(root: Path, *args: str) -> str:
    result = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if result.returncode != 0:
        raise ReleaseError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def git_ok(root: Path, *args: str) -> bool:
    return subprocess.run(["git", *args], cwd=root, capture_output=True).returncode == 0


def require_version(version: str) -> None:
    if not versions.SEMVER.match(version):
        raise ReleaseError(f"'{version}' is not a version like 0.1.0 or 1.0.0-rc.1")


def tag_version(tag: str) -> str:
    if not tag.startswith("v"):
        raise ReleaseError(f"'{tag}' is not a release tag like v0.1.0")
    require_version(tag[1:])
    return tag[1:]


def precedence(version: str) -> tuple:
    """A sort key in Semantic Versioning 2.0.0 precedence order (section 11)."""
    core, _, pre = version.partition("-")
    numbers = tuple(int(part) for part in core.split("."))
    if not pre:
        return (*numbers, 1, ())
    ids = tuple((0, int(i), "") if i.isdigit() else (1, 0, i) for i in pre.split("."))
    return (*numbers, 0, ids)


def require_clean(root: Path) -> None:
    if git(root, "status", "--porcelain", "--untracked-files=normal"):
        raise ReleaseError("the working tree has changes; commit or stash them first")


def release_tags(root: Path) -> list[str]:
    """Versions of the `v<semver>` tags in this clone, lowest first."""
    found = [t[1:] for t in git(root, "tag", "--list", "v*").split() if versions.SEMVER.match(t[1:])]
    return sorted(found, key=precedence)


def previous_release(root: Path, version: str) -> str | None:
    lower = [v for v in release_tags(root) if precedence(v) < precedence(version)]
    return lower[-1] if lower else None


def next_version(root: Path, base: str) -> str:
    """The version `next` carries at `base`."""
    manifest = tomllib.loads(git(root, "show", f"{base}:Cargo.toml"))
    return manifest["workspace"]["package"]["version"]


def require_main_in_next(root: Path) -> None:
    """Every file main changed since it last met next must read the same on next, or the release
    PR would bring a change in unreviewed (or undo it on main)."""
    main, nxt = f"{REMOTE}/main", f"{REMOTE}/next"
    base = git(root, "merge-base", main, nxt)
    changed = git(root, "diff", "--name-only", base, main).split()
    if not changed:
        return
    differ = git(root, "diff", "--name-only", nxt, main, "--", *changed).split()
    if differ:
        raise ReleaseError(f"{main} has changes {nxt} does not ({', '.join(differ[:5])}); bring "
                           "them into next through a PR first")


def attribution_hits(root: Path, text: str) -> str | None:
    """The attribution lines in `text` (AGENTS.md non-negotiable 1), or None when there are none."""
    script = f'. "{root / ATTRIBUTION}"; attribution_hits'
    result = subprocess.run(["bash", "-c", script], input=text, capture_output=True, text=True)
    if result.returncode == 1:
        return None
    return result.stdout.strip() or "the attribution check could not run"


def generate_notes(root: Path, version: str, previous: str | None, base: str) -> str:
    """The CHANGELOG.md section for `version`: the commits after `previous`'s tag up to `base`."""
    span = f"v{previous}..{base}" if previous else base
    command = ["git-cliff", "--offline", "--config", str(root / CLIFF),
               "--repository", str(root), "--tag", f"v{version}", "--", span]
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    except FileNotFoundError as error:
        raise ReleaseError("git-cliff is not installed; run inside `nix develop`") from error
    if result.returncode != 0:
        raise ReleaseError(f"git-cliff failed: {result.stderr.strip()}")
    notes = result.stdout.strip()
    if not re.search(r"^- ", notes, re.M):
        raise ReleaseError(f"no Conventional Commit to release in {span}")
    hits = attribution_hits(root, notes)
    if hits:
        raise ReleaseError(f"the notes carry AI attribution; fix the commit subject on next:\n{hits}")
    return notes + "\n"


def has_section(changelog: str, version: str) -> bool:
    return re.search(rf"^## v{re.escape(version)}\s*$", changelog, re.M) is not None


def add_section(changelog: str | None, version: str, section: str) -> str:
    """CHANGELOG.md with `section` above the newest release, or a new file holding only it."""
    text = changelog if changelog is not None else CHANGELOG_HEADER
    if has_section(text, version):
        raise ReleaseError(f"{CHANGELOG} already has a section for v{version}")
    newest = re.search(r"^## ", text, re.M)
    if newest:
        return text[:newest.start()] + section + "\n" + text[newest.start():]
    return text.rstrip("\n") + "\n\n" + section


def release_section(changelog: str, version: str) -> str:
    """The notes under `## v<version>` without the heading."""
    heading = re.search(rf"^## v{re.escape(version)}\s*$", changelog, re.M)
    if not heading:
        raise ReleaseError(f"{CHANGELOG} has no section for v{version}")
    end = re.compile(r"^## ", re.M).search(changelog, heading.end())
    body = changelog[heading.end():end.start() if end else len(changelog)].strip()
    if not body:
        raise ReleaseError(f"{CHANGELOG}'s section for v{version} is empty")
    return body + "\n"


def write_notes(root: Path, version: str) -> int:
    """`notes`: the section for `version`, from the previous release tag up to HEAD."""
    require_version(version)
    base = git(root, "rev-parse", "HEAD")
    previous = previous_release(root, version)
    path = root / CHANGELOG
    changelog = path.read_text() if path.exists() else None
    path.write_text(add_section(changelog, version, generate_notes(root, version, previous, base)))
    since = f"after v{previous}" if previous else "in the history"
    print(f"release-notes: wrote v{version} to {CHANGELOG} (commits {since} up to {base[:12]})")
    return 0


def pr_body(root: Path, version: str, changelog: str) -> str:
    template = Template((root / PR_TEMPLATE).read_text())
    notes = release_section(changelog, version).strip()
    return template.substitute(version=f"v{version}", notes=notes)


def branch_name(base: str, version: str) -> str:
    return f"chore/{base[:7]}/release-v{version}"


def check_prepare(root: Path, version: str) -> str:
    """Every check `prepare` makes before it changes anything; returns next's commit."""
    require_version(version)
    require_clean(root)
    git(root, "fetch", "--tags", REMOTE)
    base = git(root, "rev-parse", f"{REMOTE}/next")
    current = next_version(root, base)
    if precedence(version) <= precedence(current):
        raise ReleaseError(f"{version} is not newer than {current}, the version on {REMOTE}/next")
    if git_ok(root, "rev-parse", "-q", "--verify", f"refs/tags/v{version}"):
        raise ReleaseError(f"the tag v{version} already exists")
    if git_ok(root, "cat-file", "-e", f"{base}:{CHANGELOG}") and \
            has_section(git(root, "show", f"{base}:{CHANGELOG}"), version):
        raise ReleaseError(f"{CHANGELOG} on {REMOTE}/next already has a section for v{version}")
    require_main_in_next(root)
    branch = branch_name(base, version)
    if git_ok(root, "rev-parse", "-q", "--verify", f"refs/heads/{branch}"):
        raise ReleaseError(f"the branch {branch} already exists")
    generate_notes(root, version, previous_release(root, version), base)
    return base


def commit_release(root: Path, version: str, base: str, just: str) -> str:
    """On the new branch: `just release-version`, then the version and the notes as two commits.
    Returns CHANGELOG.md's new text."""
    if subprocess.run([just, "release-version", version], cwd=root).returncode != 0:
        raise ReleaseError(f"just release-version {version} failed")
    git(root, "add", "--update", "--", ".", f":!{CHANGELOG}")
    git(root, "commit", "--quiet", "-m", f"chore(release): set the version to {version}",
        "-m", "Every manifest, both lockfiles and the OpenAPI document carry the new version "
        "(just release-version).")
    git(root, "add", "--", CHANGELOG)
    git(root, "commit", "--quiet", "-m", f"chore(release): add the v{version} release notes",
        "-m", "Written by just release-version from the commit subjects since the last release. "
        + BASE_PREFIX.format(version=version) + base + ".")
    return (root / CHANGELOG).read_text()


def prepare(root: Path, version: str, just: str) -> int:
    base = check_prepare(root, version)
    branch = branch_name(base, version)
    git(root, "switch", "--no-track", "-c", branch, base)
    try:
        changelog = commit_release(root, version, base, just)
        out = root / OUT
        out.mkdir(parents=True, exist_ok=True)
        body = out / f"v{version}-pr.md"
        body.write_text(pr_body(root, version, changelog))
    except (ReleaseError, OSError, KeyError, ValueError) as error:
        raise ReleaseError(f"{error}\nThe branch {branch} is left as it stopped; remove it with "
                           f"`git reset --hard && git switch - && git branch -D {branch}`") from error
    print_prepare_steps(version, branch, body)
    return 0


def print_prepare_steps(version: str, branch: str, body: Path) -> None:
    tag = f"v{version}"
    print(f"""release-prepare: {branch} holds the {tag} version bump and release notes.
Nothing was pushed. Read the CHANGELOG.md section; `just release` refuses a tag whose section
differs from what git-cliff writes, so leave it as generated. Then:

  1. just ci
  2. git push -u {REMOTE} {branch}
     gh pr create --base next --title "chore(release): prepare {tag}" --fill
     Squash-merge it into next as usual, keeping the commit messages in the squash body.
     Merge nothing else into next until the release PR below is merged.
  3. gh pr create --base main --head next --title "chore(release): {tag}" --body-file {body}
     Merge it with a merge commit, not a squash, and keep next (docs/adr/0030).
  4. git fetch {REMOTE} && git tag -a {tag} -m "Stepclimb {tag}" {REMOTE}/main && git push {REMOTE} {tag}
  5. git worktree add ../stepclimb-{tag} {tag}, then in it: just release {tag}""")


def tag_commit(root: Path, tag: str) -> str:
    if not git_ok(root, "rev-parse", "-q", "--verify", f"refs/tags/{tag}"):
        raise ReleaseError(f"there is no tag {tag}; tag the release PR's merge on main first")
    if git(root, "cat-file", "-t", f"refs/tags/{tag}") != "tag":
        raise ReleaseError(f"{tag} is a lightweight tag; replace it with an annotated one (git tag -a)")
    commit = git(root, "rev-parse", f"{tag}^{{commit}}")
    if git(root, "rev-parse", "HEAD") != commit:
        raise ReleaseError(f"HEAD is not {tag}; run this in a worktree of the tag "
                           f"(git worktree add ../stepclimb-{tag} {tag})")
    return commit


def released_next(root: Path, tag: str, commit: str) -> str:
    """The `next` commit the release PR merged: the tag must be a merge on main's first-parent
    line, with next as its second parent."""
    if commit not in git(root, "rev-list", "--first-parent", f"{REMOTE}/main").split():
        raise ReleaseError(f"{tag} is not on {REMOTE}/main's first-parent history; tag the release "
                           "PR's merge commit")
    if not git_ok(root, "rev-parse", "-q", "--verify", f"{commit}^2"):
        raise ReleaseError(f"{tag} is not a merge commit; release PRs into main use a merge commit "
                           "(docs/adr/0030)")
    return git(root, "rev-parse", f"{commit}^2")


def notes_base(root: Path, version: str, released: str) -> str:
    """The commit the notes end at, from the notes commit's body; nothing but the release's own
    commits may come between it and the released `next`."""
    needle = BASE_PREFIX.format(version=version)
    bodies = git(root, "log", "-F", f"--grep={needle}", "--format=%B", released)
    found = re.search(re.escape(needle) + r"([0-9a-f]{40})\.", bodies)
    if not found:
        raise ReleaseError(f"no commit on the released next says where the v{version} notes end "
                           f"(\"{needle}<commit>.\"); prepare the release with just release-prepare")
    base = found.group(1)
    subjects = git(root, "log", "--format=%h %s", f"{base}..{released}").splitlines()
    late = [s for s in subjects if not s.split(" ", 1)[1].startswith(RELEASE_SUBJECT)]
    if late:
        raise ReleaseError(f"next gained commits after the v{version} notes were written, so the "
                           f"notes leave them out: {'; '.join(late[:5])}. Prepare the release again")
    return base


def require_same_notes(root: Path, version: str, base: str) -> str:
    """CHANGELOG.md's section, refused unless git-cliff writes the same for the same commits."""
    if not (root / CHANGELOG).is_file():
        raise ReleaseError(f"v{version} has no {CHANGELOG}; prepare the release with just release-prepare")
    recorded = release_section((root / CHANGELOG).read_text(), version)
    generated = release_section(generate_notes(root, version, previous_release(root, version), base), version)
    if recorded != generated:
        diff = "".join(difflib.unified_diff(generated.splitlines(True), recorded.splitlines(True),
                                            "git-cliff", CHANGELOG))
        raise ReleaseError(f"{CHANGELOG}'s v{version} section differs from what git-cliff writes:\n{diff}")
    return recorded


def verify(root: Path, tag: str) -> int:
    version = tag_version(tag)
    require_clean(root)
    git(root, "fetch", "--tags", REMOTE)
    commit = tag_commit(root, tag)
    workspace = versions.source_version(root)
    if workspace != version:
        raise ReleaseError(f"the tag {tag} does not match the workspace version {workspace} "
                           "(Cargo.toml [workspace.package])")
    base = notes_base(root, version, released_next(root, tag, commit))
    notes = require_same_notes(root, version, base)
    out = root / OUT
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{tag}-notes.md").write_text(notes)
    print(f"release: {tag} is annotated, merged on {REMOTE}/main and matches the workspace version, "
          f"and its notes match git-cliff's; notes in {OUT / f'{tag}-notes.md'}. "
          f"Running just ci on {commit[:12]}.")
    return 0


def require_ci_pass(root: Path, commit: str) -> None:
    """Refused unless bin/ci-stamp.sh recorded a `just ci` pass on `commit` in this worktree."""
    stamp = Path(git(root, "rev-parse", "--absolute-git-dir")) / "stepclimb-ci-pass"
    if not stamp.is_file() or stamp.read_text().strip() != commit:
        raise ReleaseError(f"just ci has not passed on {commit[:12]} in this worktree")


def finish(root: Path, tag: str) -> int:
    tag_version(tag)
    commit = tag_commit(root, tag)
    require_ci_pass(root, commit)
    notes = root / OUT / f"{tag}-notes.md"
    if not notes.is_file():
        raise ReleaseError(f"{notes} is missing; run just release {tag} again")
    print(f"release: just ci passed on {tag} ({commit[:12]}); building the release artifacts "
          "(bin/release_artifacts.py build)")
    return 0


def main(argv: list[str]) -> int:
    root = Path.cwd()
    try:
        if len(argv) == 2 and argv[0] == "notes":
            return write_notes(root, argv[1])
        if len(argv) == 3 and argv[0] == "prepare":
            return prepare(root, argv[1], argv[2])
        if len(argv) == 2 and argv[0] in ("verify", "finish"):
            return (verify if argv[0] == "verify" else finish)(root, argv[1])
        print(__doc__.rsplit("Usage: ", 1)[1].strip(), file=sys.stderr)
        return 2
    except ReleaseError as error:
        print(f"release: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
