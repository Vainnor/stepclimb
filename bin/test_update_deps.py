# SPDX-License-Identifier: AGPL-3.0-or-later
"""The monthly dependency update (bin/update_deps.py; docs/specs/m0/01-repo-and-ci.md B13).
Nothing here reaches the network: crates.io answers come from a fake fetch, and `cargo`,
`docker buildx imagetools` and `git` from a fake `run` working on scratch lockfiles and files."""

from __future__ import annotations

import shutil
import sys
import tempfile
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import update_deps as u  # noqa: E402

NOW = datetime(2026, 11, 14, 12, 0, tzinfo=UTC)
CUTOFF = NOW - u.MIN_AGE
DIGEST_A = "sha256:" + "a" * 64
DIGEST_B = "sha256:" + "b" * 64
DIGEST_C = "sha256:" + "c" * 64
INDEX = "registry+https://github.com/rust-lang/crates.io-index"


def lock(*packages: tuple[str, str]) -> str:
    """A Cargo.lock with one workspace crate and the given registry crates."""
    blocks = ['version = 4\n\n[[package]]\nname = "stepclimb-core"\nversion = "0.0.0"\n']
    blocks += [f'[[package]]\nname = "{n}"\nversion = "{v}"\nsource = "{INDEX}"\n' for n, v in packages]
    return "\n".join(blocks)


def days_ago(days: float) -> datetime:
    return NOW - timedelta(days=days)


def published(*entries: tuple) -> list[u.Published]:
    return [u.Published(e[0], days_ago(e[1]), bool(e[2]) if len(e) > 2 else False) for e in entries]


class Lockfiles(unittest.TestCase):
    def test_reads_registry_crates_and_skips_workspace_crates(self) -> None:
        text = lock(("serde", "1.0.200"), ("syn", "1.0.109"), ("syn", "2.0.80"))
        self.assertEqual(u.registry_packages(text), {"serde": {"1.0.200"}, "syn": {"1.0.109", "2.0.80"}})

    def test_lists_changed_added_and_removed_crates(self) -> None:
        before = {"serde": {"1.0.200"}, "syn": {"2.0.80"}, "gone": {"0.1.0"}}
        after = {"serde": {"1.0.210"}, "syn": {"2.0.80"}, "new": {"3.0.0"}}
        self.assertEqual(u.lock_changes(before, after), ["-gone 0.1.0", "+new 3.0.0", "serde 1.0.200 -> 1.0.210"])


class Versions(unittest.TestCase):
    def test_compatibility_follows_cargo_caret_rules(self) -> None:
        self.assertTrue(u.compatible("1.9.0", "1.2.0"))
        self.assertFalse(u.compatible("2.0.0", "1.2.0"))
        self.assertTrue(u.compatible("0.4.9", "0.4.1"))
        self.assertFalse(u.compatible("0.5.0", "0.4.1"))
        self.assertFalse(u.compatible("0.0.4", "0.0.3"))

    def test_picks_the_newest_version_at_least_14_days_old(self) -> None:
        versions = published(("1.0.200", 60), ("1.0.205", 20), ("1.0.208", 14), ("1.0.210", 2))
        self.assertEqual(u.newest_old_enough(versions, "1.0.210", CUTOFF), "1.0.208")

    def test_a_version_one_minute_short_of_14_days_is_too_new(self) -> None:
        versions = published(("1.0.200", 60), ("1.0.205", 14 - 1 / 1440), ("1.0.210", 2))
        self.assertEqual(u.newest_old_enough(versions, "1.0.210", CUTOFF), "1.0.200")

    def test_skips_yanked_pre_release_and_incompatible_versions(self) -> None:
        versions = published(
            ("1.0.200", 60), ("1.0.206", 30, True), ("1.0.207-rc.1", 30), ("2.0.0", 30), ("1.0.210", 2)
        )
        self.assertEqual(u.newest_old_enough(versions, "1.0.210", CUTOFF), "1.0.200")

    def test_stays_in_the_series_when_two_series_are_locked(self) -> None:
        versions = published(("0.52.0", 900), ("0.59.0", 300), ("0.61.0", 100), ("0.61.2", 3))
        self.assertEqual(u.newest_old_enough(versions, "0.61.2", CUTOFF), "0.61.0")
        self.assertEqual(u.newest_old_enough(versions, "0.52.0", CUTOFF), "0.52.0")

    def test_none_when_the_series_has_nothing_old_enough(self) -> None:
        self.assertIsNone(u.newest_old_enough(published(("1.0.100", 90), ("3.2.0", 3)), "3.2.0", CUTOFF))


class FakeCargo:
    """Stands in for `run`: `cargo update` writes `updated` into the lockfile, and
    `cargo update -p name@old --precise new` rewrites that entry, after `effects` (name ->
    function of the lockfile text) or refuses with `refusals` (name -> Cargo's message)."""

    def __init__(self, lock_path: Path, updated: str) -> None:
        self.lock_path, self.updated = lock_path, updated
        self.commands: list[list[str]] = []
        self.refusals: dict[str, str] = {}
        self.effects: dict[str, object] = {}

    def __call__(self, command: list[str], cwd: Path | None = None) -> str:
        self.commands.append(command)
        if "--precise" not in command:
            self.lock_path.write_text(self.updated)
            return ""
        spec, target = command[command.index("-p") + 1], command[command.index("--precise") + 1]
        name, old = spec.split("@")
        if name in self.refusals:
            raise u.UpdateError(self.refusals[name])
        text = self.lock_path.read_text()
        self.assertion(f'name = "{name}"\nversion = "{old}"' in text, f"{spec} is not in the lockfile")
        text = text.replace(f'name = "{name}"\nversion = "{old}"', f'name = "{name}"\nversion = "{target}"')
        if name in self.effects:
            text = self.effects[name](text)
        self.lock_path.write_text(text)
        return ""

    @staticmethod
    def assertion(ok: bool, message: str) -> None:
        if not ok:
            raise AssertionError(message)

    def precise(self) -> list[str]:
        return [" ".join(c[c.index("-p"):]) for c in self.commands if "--precise" in c]


class PinBack(unittest.TestCase):
    CRATES = {
        "serde": published(("1.0.200", 60), ("1.0.205", 20), ("1.0.210", 2)),
        "windows-sys": published(("0.52.0", 900), ("0.61.0", 100), ("0.61.2", 3)),
        "jiff": published(("0.2.40", 30), ("0.2.43", 3), ("0.2.44", 0.5)),
        "jiff-static": published(("0.2.40", 30), ("0.2.44", 0.5)),
        "fresh": published(("1.0.0", 2)),
        "wry": published(("0.54.0", 40), ("0.54.2", 5)),
    }

    def setUp(self) -> None:
        self.dir = Path(tempfile.mkdtemp(prefix="update-deps-"))
        self.addCleanup(shutil.rmtree, self.dir)
        self.lock = self.dir / "Cargo.lock"
        self.manifest = str(self.dir / "Cargo.toml")
        self.fetched: list[str] = []

    def crates(self) -> u.Crates:
        def fetch(name: str) -> list[u.Published]:
            self.fetched.append(name)
            return self.CRATES[name]
        return u.Crates(fetch)

    def update(self, before: str, after: str) -> tuple[FakeCargo, list[str]]:
        self.lock.write_text(before)
        cargo = FakeCargo(self.lock, after)
        with mock.patch.object(u, "run", cargo), mock.patch("builtins.print"):
            problems = u.update_cargo(self.manifest, NOW, self.crates())
        return cargo, problems

    def test_sets_young_versions_back_in_their_series_and_keeps_old_ones(self) -> None:
        before = lock(("serde", "1.0.200"), ("windows-sys", "0.52.0"), ("windows-sys", "0.61.0"))
        after = lock(("serde", "1.0.210"), ("windows-sys", "0.52.0"), ("windows-sys", "0.61.2"))
        cargo, problems = self.update(before, after)
        self.assertEqual(problems, [])
        self.assertEqual(cargo.commands[0], ["cargo", "update", "--manifest-path", self.manifest])
        self.assertEqual(cargo.precise(), ["-p serde@1.0.210 --precise 1.0.205",
                                           "-p windows-sys@0.61.2 --precise 0.61.0"])
        self.assertEqual(u.registry_packages(self.lock.read_text()),
                         {"serde": {"1.0.205"}, "windows-sys": {"0.52.0", "0.61.0"}})

    def test_also_sets_back_a_young_version_the_lockfile_already_had(self) -> None:
        cargo, problems = self.update(lock(("jiff", "0.2.43")), lock(("jiff", "0.2.43")))
        self.assertEqual(problems, [])
        self.assertEqual(cargo.precise(), ["-p jiff@0.2.43 --precise 0.2.40"])

    def test_reads_the_lockfile_again_after_each_pin(self) -> None:
        after = lock(("jiff", "0.2.44"), ("jiff-static", "0.2.44"))
        self.lock.write_text(lock(("jiff", "0.2.40"), ("jiff-static", "0.2.40")))
        cargo = FakeCargo(self.lock, after)
        cargo.effects["jiff"] = lambda text: text.replace(
            'name = "jiff-static"\nversion = "0.2.44"', 'name = "jiff-static"\nversion = "0.2.40"')
        with mock.patch.object(u, "run", cargo), mock.patch("builtins.print"):
            problems = u.update_cargo(self.manifest, NOW, self.crates())
        self.assertEqual(problems, [])
        self.assertEqual(cargo.precise(), ["-p jiff@0.2.44 --precise 0.2.40"])

    def test_fails_naming_a_pin_cargo_refuses(self) -> None:
        self.lock.write_text(lock(("wry", "0.54.2")))
        cargo = FakeCargo(self.lock, lock(("wry", "0.54.2")))
        cargo.refusals["wry"] = "failed to select a version for the requirement `wry = \"^0.54.2\"`"
        with mock.patch.object(u, "run", cargo), mock.patch("builtins.print"):
            problems = u.update_cargo(self.manifest, NOW, self.crates())
        self.assertEqual(len(problems), 1)
        self.assertIn("wry 0.54.2 was published less than 14 days ago; cargo refused 0.54.0:", problems[0])
        self.assertIn("requirement `wry = \"^0.54.2\"`", problems[0])
        self.assertEqual(cargo.precise(), ["-p wry@0.54.2 --precise 0.54.0"])

    def test_tries_a_refused_pin_again_after_a_later_pin_lowers_its_requirement(self) -> None:
        self.lock.write_text(lock(("jiff-static", "0.2.44"), ("wry", "0.54.2")))
        cargo = FakeCargo(self.lock, lock(("jiff-static", "0.2.44"), ("wry", "0.54.2")))
        cargo.refusals["jiff-static"] = "required by wry 0.54.2"

        def wry_pinned(text: str) -> str:
            del cargo.refusals["jiff-static"]
            return text
        cargo.effects["wry"] = wry_pinned
        with mock.patch.object(u, "run", cargo), mock.patch("builtins.print"):
            problems = u.update_cargo(self.manifest, NOW, self.crates())
        self.assertEqual(problems, [])
        self.assertEqual(cargo.precise(), ["-p jiff-static@0.2.44 --precise 0.2.40",
                                           "-p wry@0.54.2 --precise 0.54.0",
                                           "-p jiff-static@0.2.44 --precise 0.2.40"])

    def test_fails_a_crate_with_no_version_old_enough(self) -> None:
        cargo, problems = self.update(lock(), lock(("fresh", "1.0.0")))
        self.assertEqual(cargo.precise(), [])
        self.assertEqual(len(problems), 1)
        self.assertIn("fresh 1.0.0 was published less than 14 days ago; no version of its series", problems[0])

    def test_looks_each_crate_up_once(self) -> None:
        self.update(lock(("serde", "1.0.200")), lock(("serde", "1.0.210"), ("jiff", "0.2.43")))
        self.assertEqual(sorted(self.fetched), ["jiff", "serde"])

    def test_names_the_crate_when_crates_io_fails(self) -> None:
        def broken(request, timeout):
            raise u.urllib.error.URLError("no route to host")
        with mock.patch.object(u.urllib.request, "urlopen", broken):
            with self.assertRaisesRegex(u.UpdateError, "crates.io lookup of serde failed: .*no route"):
                u.fetch_published("serde")


class FakeRegistry:
    """Stands in for `run` for `docker buildx imagetools inspect`: `tags` maps name:tag to its
    current digest, `builds` maps a digest to its build date."""

    def __init__(self, tags: dict[str, str], builds: dict[str, str]) -> None:
        self.tags, self.builds = tags, builds

    def __call__(self, command: list[str], cwd: Path | None = None) -> str:
        ref, template = command[4], command[6]
        if template == "{{json .Manifest.Digest}}":
            return f'"{self.tags[ref]}"'
        digest = ref.split("@", 1)[1]
        return f'{{"linux/amd64": {{"created": "{self.builds[digest]}"}}}}'


class Images(unittest.TestCase):
    def test_reads_pinned_references(self) -> None:
        text = (
            f"FROM --platform=$BUILDPLATFORM node:26.13.0-bookworm-slim@{DIGEST_A} AS web\n"
            f"    image: cgr.dev/chainguard/minio:latest@{DIGEST_B}\n"
            "    image: stepclimb:dev\n"
        )
        self.assertEqual(
            u.image_refs(text),
            {"node:26.13.0-bookworm-slim": DIGEST_A, "cgr.dev/chainguard/minio:latest": DIGEST_B},
        )

    def test_finds_every_image_this_repository_pins(self) -> None:
        root = Path(__file__).resolve().parent.parent
        refs = {}
        for path in u.IMAGE_FILES:
            refs.update(u.image_refs((root / path).read_text()))
        self.assertGreaterEqual(len(refs), 5, refs)

    def test_moves_only_to_a_digest_built_14_days_ago_or_more(self) -> None:
        self.assertEqual(u.image_decision(DIGEST_A, DIGEST_A, days_ago(1), NOW), "unchanged")
        self.assertEqual(u.image_decision(DIGEST_A, DIGEST_B, days_ago(14), NOW), "update")
        self.assertEqual(u.image_decision(DIGEST_A, DIGEST_B, days_ago(13.9), NOW), "too-new")

    def test_keeps_a_digest_whose_build_date_is_a_placeholder(self) -> None:
        epoch = datetime(1970, 1, 1, tzinfo=UTC)
        self.assertEqual(u.image_decision(DIGEST_A, DIGEST_B, epoch, NOW), "unknown-age")

    def test_reads_the_amd64_build_date_with_nanoseconds(self) -> None:
        multi = {"linux/arm64": {"created": "2026-10-01T00:00:00Z"},
                 "linux/amd64": {"created": "2026-10-21T21:32:33.967060949Z"}}
        self.assertEqual(u.build_date(multi), datetime(2026, 10, 21, 21, 32, 33, 967060, tzinfo=UTC))
        self.assertEqual(u.build_date({"created": "2026-10-21T21:32:33Z"}),
                         datetime(2026, 10, 21, 21, 32, 33, tzinfo=UTC))
        with self.assertRaises(u.UpdateError):
            u.build_date({"linux/arm64": {"created": "2026-10-01T00:00:00Z"}})

    def test_writes_old_enough_digests_into_every_file_and_keeps_the_rest(self) -> None:
        tmp = Path(tempfile.mkdtemp(prefix="update-images-"))
        self.addCleanup(shutil.rmtree, tmp)
        dockerfile, compose = tmp / "Dockerfile", tmp / "compose.yaml"
        dockerfile.write_text(f"FROM node:26@{DIGEST_A}\nFROM gcr.io/distroless/cc:nonroot@{DIGEST_A}\n")
        compose.write_text(f"  image: postgres:16@{DIGEST_A}\n  image: cgr.dev/x/minio:latest@{DIGEST_A}\n"
                           f"  image: node:26@{DIGEST_A}\n")
        registry = FakeRegistry(
            tags={"node:26": DIGEST_B, "postgres:16": DIGEST_A, "cgr.dev/x/minio:latest": DIGEST_C,
                  "gcr.io/distroless/cc:nonroot": DIGEST_B},
            builds={DIGEST_A: "2026-09-01T00:00:00Z", DIGEST_B: "2026-10-30T12:00:00.123456789Z",
                    DIGEST_C: "2026-11-14T00:00:00Z"},
        )
        lines: list[str] = []
        with mock.patch.object(u, "run", registry), mock.patch("builtins.print", lambda *a, **k: lines.append(" ".join(a))):
            kept = u.update_images(NOW, (str(dockerfile), str(compose)))
        self.assertEqual(dockerfile.read_text().count(DIGEST_B), 2)
        self.assertIn(f"node:26@{DIGEST_B}", compose.read_text())
        self.assertIn(f"postgres:16@{DIGEST_A}", compose.read_text())
        self.assertIn(f"minio:latest@{DIGEST_A}", compose.read_text())
        self.assertEqual(kept, [f"cgr.dev/x/minio:latest: kept {DIGEST_A[:19]} (74 days old); the tag's build is too-new"])
        self.assertIn("  postgres:16: current; the pinned build is 74 days old", lines)

    def test_keeps_a_pin_whose_newest_build_has_no_date(self) -> None:
        tmp = Path(tempfile.mkdtemp(prefix="update-images-"))
        self.addCleanup(shutil.rmtree, tmp)
        dockerfile = tmp / "Dockerfile"
        dockerfile.write_text(f"FROM gcr.io/distroless/cc:nonroot@{DIGEST_A}\n")
        registry = FakeRegistry({"gcr.io/distroless/cc:nonroot": DIGEST_B},
                                {DIGEST_A: "1970-01-01T00:00:00Z", DIGEST_B: "1970-01-01T00:00:00Z"})
        with mock.patch.object(u, "run", registry), mock.patch("builtins.print"):
            kept = u.update_images(NOW, (str(dockerfile),))
        self.assertEqual(dockerfile.read_text(), f"FROM gcr.io/distroless/cc:nonroot@{DIGEST_A}\n")
        self.assertEqual(kept, [f"gcr.io/distroless/cc:nonroot: kept {DIGEST_A[:19]} (build date unknown); "
                                "the tag's build is unknown-age"])


class Result(unittest.TestCase):
    def test_reads_porcelain_z_with_renames_and_spaces(self) -> None:
        out = " M Cargo.lock\0R  new name.txt\0old.txt\0?? x y.txt\0"
        self.assertEqual(u.status_paths(out), ["Cargo.lock", "new name.txt", "old.txt", "x y.txt"])
        self.assertEqual(u.status_paths(""), [])

    def test_only_lockfiles_and_image_files_may_change(self) -> None:
        paths = ["Cargo.lock", "pnpm-lock.yaml", "deploy/Dockerfile", "web/package.json", "x.txt"]
        self.assertEqual(u.unexpected_changes(paths), ["web/package.json", "x.txt"])

    def test_a_failed_step_says_how_to_start_again(self) -> None:
        def failing(command, cwd=None):
            if command[:2] == ["git", "status"]:
                return ""
            raise u.UpdateError("cargo update failed: network down")
        errors: list[str] = []
        with mock.patch.object(u, "run", failing), \
                mock.patch.object(u.sys, "stderr", mock.Mock(write=errors.append)), \
                mock.patch("builtins.print", lambda *a, file=None, **k: errors.append(" ".join(a))):
            self.assertEqual(u.main(), 1)
        text = "\n".join(errors)
        self.assertIn("cargo update failed: network down", text)
        self.assertIn("git checkout -- Cargo.lock apps/app/src-tauri/Cargo.lock pnpm-lock.yaml", text)


if __name__ == "__main__":
    unittest.main()
