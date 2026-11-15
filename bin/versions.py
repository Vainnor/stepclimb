# SPDX-License-Identifier: AGPL-3.0-or-later
"""One version for the server, the web app and the Tauri app (docs/specs/m0/01-repo-and-ci.md B14).

The source is `[workspace.package] version` in Cargo.toml. `check` fails when any of these
differs:

- each workspace member from `cargo metadata --no-deps`, whose manifest must say
  `version.workspace = true`, and its entry in Cargo.lock;
- apps/app/src-tauri/Cargo.toml, its Cargo.lock entry and tauri.conf.json;
- openapi/stepclimb.json `info.version`, which utoipa takes from the server crate's version;
- any package.json with a `version` that Changesets does not own. The packages Changesets
  versions (those not in .changeset/config.json `ignore`) keep their own number (ADR 0015).

`set <x.y.z>` writes the value into every manifest `check` reads; `just release-version` then
refreshes the lockfiles. Any error, including a file that cannot be read, fails the run.

Usage: versions.py check [root] | versions.py set <version> [root]"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

APP = "apps/app/src-tauri"
OPENAPI = "openapi/stepclimb.json"
# Semantic Versioning 2.0.0: MAJOR.MINOR.PATCH and an optional pre-release; no build metadata.
# A numeric identifier, in the core or the pre-release, has no leading zero (section 9).
_PRE_ID = r"(0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(rf"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-{_PRE_ID}(\.{_PRE_ID})*)?$")
OPENAPI_VERSION = re.compile(r'^(    "version": )"[^"]*"', re.M)
TOML_VERSION = re.compile(r'^version\s*=\s*"[^"]*"', re.M)
JSON_VERSION = re.compile(r'^(  "version": )"[^"]*"', re.M)


class VersionError(Exception):
    """The repository cannot be checked or written; the message says why."""


def read_toml(path: Path) -> dict:
    try:
        return tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise VersionError(f"cannot read {path}: {error}") from error


def read_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise VersionError(f"cannot read {path}: {error}") from error


def source_version(root: Path) -> str:
    version = read_toml(root / "Cargo.toml").get("workspace", {}).get("package", {}).get("version")
    if not version:
        raise VersionError("Cargo.toml has no [workspace.package] version")
    return version


def workspace_members(root: Path) -> list[Path]:
    """The root workspace's member manifests, as Cargo resolves them."""
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1"],
        cwd=root, capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise VersionError(f"cargo metadata failed: {result.stderr.strip()}")
    packages = json.loads(result.stdout)["packages"]
    return sorted(Path(p["manifest_path"]).relative_to(root.resolve()) for p in packages)


def lock_versions(lock: Path) -> dict[str, set[str]]:
    versions: dict[str, set[str]] = {}
    for package in read_toml(lock).get("package", []):
        versions.setdefault(package["name"], set()).add(package["version"])
    return versions


def tracked_package_jsons(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--", "package.json", "*/package.json"],
        cwd=root, capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise VersionError(f"git ls-files failed: {result.stderr.strip()}")
    return [Path(p) for p in result.stdout.split("\0") if p]


def owned_package_jsons(root: Path) -> list[tuple[Path, dict]]:
    """package.json files outside Changesets that carry a version, with their contents."""
    ignored = read_json(root / ".changeset" / "config.json").get("ignore")
    if not isinstance(ignored, list):
        raise VersionError(".changeset/config.json has no \"ignore\" list")
    owned = []
    for path in tracked_package_jsons(root):
        data = read_json(root / path)
        if "version" in data and (str(path) == "package.json" or data.get("name") in ignored):
            owned.append((path, data))
    return owned


def places(root: Path) -> list[tuple[str, str]]:
    """Every (place, version) that must equal the source, plus rule failures as version None."""
    found: list[tuple[str, str | None]] = []
    root_lock = lock_versions(root / "Cargo.lock")
    for manifest in workspace_members(root):
        package = read_toml(root / manifest).get("package", {})
        if package.get("version") != {"workspace": True}:
            found.append((f"{manifest} must say version.workspace = true", None))
        name = package.get("name", "?")
        found.append((f"Cargo.lock ({name})", ", ".join(sorted(root_lock.get(name, set())))))
    app = read_toml(root / APP / "Cargo.toml").get("package", {})
    found.append((f"{APP}/Cargo.toml [package]", app.get("version", "")))
    app_lock = lock_versions(root / APP / "Cargo.lock")
    found.append((f"{APP}/Cargo.lock ({app.get('name')})", ", ".join(sorted(app_lock.get(app.get("name"), set())))))
    found.append((f"{APP}/tauri.conf.json", read_json(root / APP / "tauri.conf.json").get("version", "")))
    found.append((f"{OPENAPI} info.version", read_json(root / OPENAPI).get("info", {}).get("version", "")))
    found += [(str(path), data["version"]) for path, data in owned_package_jsons(root)]
    return found


def check(root: Path) -> int:
    want = source_version(root)
    found = places(root)
    failures = []
    for place, version in found:
        if version is None:
            failures.append(f"version-check: {place}")
        elif version != want:
            failures.append(f"version-check: {place} has '{version or '<none>'}', Cargo.toml "
                            f"[workspace.package] has '{want}'; run just release-version <x.y.z>")
    for line in failures:
        print(line, file=sys.stderr)
    if failures:
        print(f"version-check: FAILED ({len(failures)} of {len(found)} places)", file=sys.stderr)
        return 1
    print(f"version-check: ok ({want} in {len(found)} places)")
    return 0


def replace_once(pattern: re.Pattern[str], text: str, new: str, where: str) -> str:
    text, count = pattern.subn(new, text, count=1)
    if count != 1:
        raise VersionError(f"{where} has no version line to edit")
    return text


def set_toml(path: Path, section: str, version: str) -> None:
    """Rewrites the `version = "..."` line inside `[section]`, leaving the rest of the file."""
    text = path.read_text()
    header = re.search(rf"^\[{re.escape(section)}\]\s*$", text, re.M)
    if not header:
        raise VersionError(f"{path} has no [{section}]")
    end = re.compile(r"^\[", re.M).search(text, header.end())
    body = text[header.end():end.start() if end else len(text)]
    body = replace_once(TOML_VERSION, body, f'version = "{version}"', f"{path} [{section}]")
    path.write_text(text[:header.end()] + body + text[end.start() if end else len(text):])


def set_json(path: Path, version: str, pattern: re.Pattern[str] = JSON_VERSION) -> None:
    """Rewrites the `"version"` line `pattern` finds (top level by default) so the file keeps its
    formatting."""
    text = replace_once(pattern, path.read_text(), rf'\g<1>"{version}"', str(path))
    json.loads(text)
    path.write_text(text)


def set_openapi(path: Path, version: str) -> None:
    """Rewrites `info.version`, the first four-space `"version"` line, and checks that it was."""
    set_json(path, version, OPENAPI_VERSION)
    if read_json(path).get("info", {}).get("version") != version:
        raise VersionError(f"{path}: the first nested \"version\" line is not info.version")


def set_version(root: Path, version: str) -> int:
    if not SEMVER.match(version):
        print(f"set-version: '{version}' is not a version like 0.1.0 or 1.0.0-rc.1", file=sys.stderr)
        return 2
    owned = owned_package_jsons(root)
    set_toml(root / "Cargo.toml", "workspace.package", version)
    set_toml(root / APP / "Cargo.toml", "package", version)
    set_json(root / APP / "tauri.conf.json", version)
    set_openapi(root / OPENAPI, version)
    for path, _ in owned:
        set_json(root / path, version)
    print(f"set-version: wrote {version}")
    return 0


def main(argv: list[str]) -> int:
    try:
        if argv[:1] == ["check"] and len(argv) <= 2:
            return check(Path(argv[1] if len(argv) > 1 else "."))
        if argv[:1] == ["set"] and len(argv) in (2, 3):
            return set_version(Path(argv[2] if len(argv) > 2 else "."), argv[1])
        print(__doc__.rsplit("Usage: ", 1)[1].strip(), file=sys.stderr)
        return 2
    except VersionError as error:
        print(f"version-check: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
