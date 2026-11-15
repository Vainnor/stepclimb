# SPDX-License-Identifier: AGPL-3.0-or-later
"""The monthly dependency update (docs/specs/m0/01-repo-and-ci.md B13), run by `just update-deps`.

Updates within the pinned versions only, so the diff is lockfiles and image digests:

1. `cargo update` in both Cargo workspaces. Stable Cargo has no minimum release age (it is the
   nightly-only `-Z min-publish-age`), so every crate version in each lockfile afterwards, new or
   not, is looked up on crates.io. One published less than 14 days ago is set back with
   `cargo update -p name@version --precise` to the newest version of the same semver series
   that is at least 14 days old. When Cargo refuses (a dependent requires the newer version), or
   no such version exists, the run fails naming the crate and Cargo's reason.
2. `pnpm update -r`, which keeps every exact pin in package.json and refreshes the lockfile;
   pnpm itself refuses versions younger than `minimumReleaseAge` (pnpm-workspace.yaml).
3. Each base and Compose image pin moves to its tag's current digest once that build is at least
   14 days old. A registry lists only a tag's current digest, not its earlier ones, so a tag
   rebuilt more often than every 14 days never moves; each run prints how many days old the
   pinned build is, so a pin held back for months shows up.

It refuses to start on a dirty tree, fails when the result touches any other file, and on a
failure prints how to put the lockfiles back."""

from __future__ import annotations

import json
import re
import subprocess
import sys
import time
import tomllib
import urllib.error
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path

MIN_AGE = timedelta(days=14)
CARGO_WORKSPACES = ("Cargo.toml", "apps/app/src-tauri/Cargo.toml")
LOCKFILES = ("Cargo.lock", "apps/app/src-tauri/Cargo.lock", "pnpm-lock.yaml")
IMAGE_FILES = ("deploy/Dockerfile", "deploy/compose/compose.yaml", "deploy/compose/compose.production.yaml")
IMAGE_REF = re.compile(r"(?P<ref>[a-z0-9][a-z0-9./_-]*:[A-Za-z0-9._-]+)@(?P<digest>sha256:[0-9a-f]{64})")
CRATES_IO = "https://crates.io/api/v1/crates/{name}/versions"
USER_AGENT = "update-deps-lockfile-age-check"
# A build date before this is a placeholder (reproducible images report 1970-01-01), not an age.
EARLIEST_REAL_BUILD = datetime(2000, 1, 1, tzinfo=UTC)


class UpdateError(Exception):
    """A step that needs a person: its message says which and why."""


# --- Cargo lockfiles -------------------------------------------------------------------------


def registry_packages(lock_text: str) -> dict[str, set[str]]:
    """Crate name -> versions from crates.io (registry sources) in a Cargo.lock."""
    packages: dict[str, set[str]] = {}
    for package in tomllib.loads(lock_text).get("package", []):
        if str(package.get("source", "")).startswith(("registry+", "sparse+")):
            packages.setdefault(package["name"], set()).add(package["version"])
    return packages


def lock_changes(before: dict[str, set[str]], after: dict[str, set[str]]) -> list[str]:
    """One line per crate whose versions changed: `name old -> new`, `+name v`, `-name v`."""
    lines = []
    for name in sorted(before.keys() | after.keys()):
        old, new = before.get(name, set()), after.get(name, set())
        if old == new:
            continue
        if not old:
            lines.append(f"+{name} {', '.join(sorted(new))}")
        elif not new:
            lines.append(f"-{name} {', '.join(sorted(old))}")
        else:
            lines.append(f"{name} {', '.join(sorted(old - new))} -> {', '.join(sorted(new - old))}")
    return lines


def all_versions(packages: dict[str, set[str]]) -> list[tuple[str, str]]:
    return sorted((name, version) for name, versions in packages.items() for version in versions)


# --- Versions --------------------------------------------------------------------------------

SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$")


def parse_version(text: str) -> tuple[int, int, int, str]:
    match = SEMVER.match(text)
    if not match:
        raise UpdateError(f"cannot read the version {text!r}")
    return int(match[1]), int(match[2]), int(match[3]), match[4] or ""


def release_key(text: str) -> tuple[int, int, int, int, str]:
    """Orders versions; a pre-release sorts before its release."""
    major, minor, patch, pre = parse_version(text)
    return major, minor, patch, 0 if pre else 1, pre


def compatible(version: str, wanted: str) -> bool:
    """Whether `version` is in `wanted`'s semver series (Cargo's caret rule): same major, or for
    0.x the same minor, or for 0.0.x the same patch."""
    a, b = parse_version(version), parse_version(wanted)
    if a[0] != b[0]:
        return False
    if a[0] == 0 and a[1] != b[1]:
        return False
    return not (a[0] == 0 and a[1] == 0 and a[2] != b[2])


@dataclass(frozen=True)
class Published:
    version: str
    created: datetime
    yanked: bool


def newest_old_enough(published: list[Published], current: str, cutoff: datetime) -> str | None:
    """The newest release in `current`'s series, no newer than `current`, published at or
    before `cutoff` and not yanked; None if there is none."""
    candidates = [
        p.version for p in published
        if not p.yanked and p.created <= cutoff
        and not parse_version(p.version)[3]
        and compatible(p.version, current)
        and release_key(p.version) <= release_key(current)
    ]
    return max(candidates, key=release_key, default=None)


def fetch_published(name: str) -> list[Published]:
    """Every version of a crate with its publication time, from the crates.io API."""
    request = urllib.request.Request(CRATES_IO.format(name=name), headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            versions = json.load(response)["versions"]
    except (urllib.error.URLError, OSError, ValueError, KeyError) as error:
        raise UpdateError(f"crates.io lookup of {name} failed: {error}") from error
    time.sleep(1)  # crates.io asks API clients for at most one request a second
    return [
        Published(v["num"], datetime.fromisoformat(v["created_at"]), bool(v["yanked"]))
        for v in versions
    ]


class Crates:
    """crates.io publication data, fetched once per crate."""

    def __init__(self, fetch: Callable[[str], list[Published]] = fetch_published) -> None:
        self.fetch = fetch
        self.known: dict[str, list[Published]] = {}

    def published(self, name: str) -> list[Published]:
        if name not in self.known:
            self.known[name] = self.fetch(name)
        return self.known[name]

    def old_enough(self, name: str, version: str, cutoff: datetime) -> bool:
        created = next((p.created for p in self.published(name) if p.version == version), None)
        return created is not None and created <= cutoff


# --- Running commands ------------------------------------------------------------------------


def run(command: list[str], cwd: Path | None = None) -> str:
    try:
        result = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    except OSError as error:
        raise UpdateError(f"{command[0]} could not run: {error}") from error
    if result.returncode != 0:
        raise UpdateError(f"{' '.join(command)} failed:\n{result.stderr.strip()}")
    return result.stdout


def status_paths(porcelain_z: str) -> list[str]:
    """The paths in `git status --porcelain -z` output; a rename lists both its paths."""
    fields = porcelain_z.split("\0")
    paths, index = [], 0
    while index < len(fields) and fields[index]:
        entry = fields[index]
        paths.append(entry[3:])
        index += 1
        if entry[0] in "RC":
            paths.append(fields[index])
            index += 1
    return paths


def changed_files() -> list[str]:
    return status_paths(run(["git", "status", "--porcelain", "-z", "--untracked-files=normal"]))


def unexpected_changes(paths: list[str]) -> list[str]:
    """Changed paths other than the lockfiles and image files."""
    allowed = set(LOCKFILES) | set(IMAGE_FILES)
    return [path for path in paths if path not in allowed]


# --- Cargo -----------------------------------------------------------------------------------


def pin_back(manifest: str, lock: Path, crates: Crates, cutoff: datetime) -> list[str]:
    """Sets every crate version in `lock` published after `cutoff` back to the newest version of
    its series published before it, one at a time, reading the lockfile again after each (one
    pin can move or drop others). A pin Cargo refused is tried again after any later pin
    succeeds, since that pin may have lowered the requirement that blocked it; pins only go
    down, so this ends. Returns a problem for each young version that remains."""
    refused: dict[tuple[str, str], str] = {}
    tried: set[tuple[str, str]] = set()
    while True:
        young = [
            entry for entry in all_versions(registry_packages(lock.read_text()))
            if entry not in tried and not crates.old_enough(*entry, cutoff)
        ]
        if not young:
            break
        name, version = young[0]
        tried.add((name, version))
        target = newest_old_enough(crates.published(name), version, cutoff)
        if target is None or target == version:
            continue
        try:
            run(["cargo", "update", "--manifest-path", manifest, "-p", f"{name}@{version}", "--precise", target])
        except UpdateError as error:
            refused[(name, version)] = f"cargo refused {target}: {error}"
            continue
        tried -= refused.keys()
        refused.clear()
    return [
        f"{lock}: {name} {version} was published less than 14 days ago; "
        + refused.get((name, version), "no version of its series is 14 days old")
        for name, version in all_versions(registry_packages(lock.read_text()))
        if not crates.old_enough(name, version, cutoff)
    ]


def update_cargo(manifest: str, now: datetime, crates: Crates) -> list[str]:
    """`cargo update`, then every version younger than 14 days pinned back. Prints the crates
    that changed and returns the problems, which fail the run."""
    lock = Path(manifest).with_name("Cargo.lock")
    before = registry_packages(lock.read_text())
    run(["cargo", "update", "--manifest-path", manifest])
    print(f"  checking {sum(map(len, registry_packages(lock.read_text()).values()))} crate versions "
          "against crates.io (about one a second)")
    problems = pin_back(manifest, lock, crates, now - MIN_AGE)
    for line in lock_changes(before, registry_packages(lock.read_text())):
        print(f"  {lock}: {line}")
    return problems


# --- Images ----------------------------------------------------------------------------------


def image_refs(text: str) -> dict[str, str]:
    """`name:tag` -> pinned digest for every `name:tag@sha256:...` in a file."""
    return {m["ref"]: m["digest"] for m in IMAGE_REF.finditer(text)}


def image_decision(current: str, newest: str, built: datetime, now: datetime) -> str:
    """`unchanged`, `update`, `too-new` (built less than 14 days ago) or `unknown-age`."""
    if newest == current:
        return "unchanged"
    if built < EARLIEST_REAL_BUILD:
        return "unknown-age"
    return "update" if now - built >= MIN_AGE else "too-new"


def build_date(image_json: dict) -> datetime:
    """The linux/amd64 build date from `imagetools inspect --format '{{json .Image}}'`, which is
    one image or a map of platform to image."""
    image = image_json if "created" in image_json else image_json.get("linux/amd64", {})
    if "created" not in image:
        raise UpdateError("the image has no linux/amd64 build date")
    # Docker writes nanoseconds; Python reads at most microseconds.
    stamp = re.sub(r"(\.\d{6})\d+", r"\1", image["created"]).replace("Z", "+00:00")
    return datetime.fromisoformat(stamp)


def age_text(built: datetime, now: datetime) -> str:
    if built < EARLIEST_REAL_BUILD:
        return "build date unknown"
    return f"{(now - built).days} days old"


def inspect(ref: str, template: str) -> object:
    out = run(["docker", "buildx", "imagetools", "inspect", ref, "--format", template])
    try:
        return json.loads(out)
    except json.JSONDecodeError as error:
        raise UpdateError(f"docker buildx imagetools inspect {ref} printed no JSON: {error}") from error


def built(ref: str, digest: str) -> datetime:
    try:
        return build_date(inspect(f"{ref}@{digest}", "{{json .Image}}"))
    except UpdateError as error:
        raise UpdateError(f"image {ref}@{digest[:19]}: {error}") from error


def update_images(now: datetime, files: tuple[str, ...] = IMAGE_FILES) -> list[str]:
    """Moves each pinned image digest to its tag's current one when that build is old enough.
    Prints each pin's age; returns a line per pin kept behind its tag."""
    texts = {path: Path(path).read_text() for path in files}
    pinned: dict[str, str] = {}
    for text in texts.values():
        pinned.update(image_refs(text))
    kept = []
    for ref, digest in sorted(pinned.items()):
        newest = inspect(ref, "{{json .Manifest.Digest}}")
        held = age_text(built(ref, digest), now)
        if newest == digest:
            print(f"  {ref}: current; the pinned build is {held}")
            continue
        newest_built = built(ref, newest)
        decision = image_decision(digest, newest, newest_built, now)
        print(f"  {ref}: {decision}; the pinned build is {held}, the tag's is {age_text(newest_built, now)}")
        if decision == "update":
            texts = {path: text.replace(f"{ref}@{digest}", f"{ref}@{newest}") for path, text in texts.items()}
        else:
            kept.append(f"{ref}: kept {digest[:19]} ({held}); the tag's build is {decision}")
    for path, text in texts.items():
        Path(path).write_text(text)
    return kept


# --- The run ---------------------------------------------------------------------------------

RESET = f"update-deps: to start again, run: git checkout -- {' '.join(LOCKFILES + IMAGE_FILES)}"


def update(now: datetime) -> tuple[list[str], list[str]]:
    """Every step; returns (kept image pins, problems)."""
    problems: list[str] = []
    crates = Crates()
    for manifest in CARGO_WORKSPACES:
        print(f"cargo update ({manifest})")
        problems += update_cargo(manifest, now, crates)
    print("pnpm update -r")
    run(["pnpm", "update", "-r"])
    print("image digests")
    kept = update_images(now)
    problems += [f"{path} changed; update-deps changes only lockfiles and image digests"
                 for path in unexpected_changes(changed_files())]
    return kept, problems


def main() -> int:
    try:
        if changed_files():
            print("update-deps: the working tree has changes; commit or stash them first", file=sys.stderr)
            return 1
        kept, problems = update(datetime.now(UTC))
        print(run(["git", "diff", "--stat"]), end="")
    except UpdateError as error:
        print(f"update-deps: {error}\n{RESET}", file=sys.stderr)
        return 1
    for line in kept:
        print(f"update-deps: {line}")
    for line in problems:
        print(f"update-deps: {line}", file=sys.stderr)
    if problems:
        print(f"update-deps: FAILED ({len(problems)})\n{RESET}", file=sys.stderr)
        return 1
    print("update-deps: done; now just ci, just deny and just app-build, then a lockfile-only PR")
    return 0


if __name__ == "__main__":
    sys.exit(main())
