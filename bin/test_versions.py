# SPDX-License-Identifier: AGPL-3.0-or-later
"""bin/check-versions.sh and bin/versions.py (docs/specs/m0/01-repo-and-ci.md B14, T6), run
against a copy of this repository's crates, manifests and lockfiles in a scratch git repository."""

from __future__ import annotations

import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
APP = "apps/app/src-tauri"


def manifests() -> list[str]:
    """Every file the two scripts read, as tracked in this checkout."""
    listed = subprocess.run(
        ["git", "ls-files", "--", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates",
         f"{APP}/Cargo.toml", f"{APP}/Cargo.lock", f"{APP}/tauri.conf.json", "openapi/stepclimb.json",
         "package.json", "*/package.json", ".changeset/config.json"],
        cwd=REPO, check=True, capture_output=True, text=True,
    ).stdout.split()
    return [path for path in listed if (REPO / path).is_file()]


class Versions(unittest.TestCase):
    def setUp(self) -> None:
        self.root = Path(tempfile.mkdtemp(prefix="versions-"))
        self.addCleanup(shutil.rmtree, self.root)
        for path in manifests():
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / path, self.root / path)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        subprocess.run(["git", "add", "-A"], cwd=self.root, check=True)
        self.version = self.read_version()

    def read_version(self) -> str:
        text = (self.root / "Cargo.toml").read_text()
        section = text.split("[workspace.package]", 1)[1]
        return re.search(r'^version = "([^"]*)"', section, re.M).group(1)

    def run_script(self, script: str, *args: str) -> subprocess.CompletedProcess[str]:
        """Runs a script with the scratch repository as its last argument (its root)."""
        if script == "set-version":
            command = ["python3", "-B", str(REPO / "bin" / "versions.py"), "set", *args, str(self.root)]
        else:
            command = ["bash", str(REPO / "bin" / script), *args, str(self.root)]
        return subprocess.run(command, capture_output=True, text=True)

    def check(self) -> subprocess.CompletedProcess[str]:
        return self.run_script("check-versions.sh")

    def edit_json(self, path: str, **fields: str) -> None:
        file = self.root / path
        data = json.loads(file.read_text())
        data.update(fields)
        file.write_text(json.dumps(data, indent=2) + "\n")

    def test_this_checkout_agrees(self) -> None:
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"version-check: ok ({self.version} in ", result.stdout)

    def test_a_changed_tauri_version_fails_naming_both_values(self) -> None:
        self.edit_json(f"{APP}/tauri.conf.json", version="9.9.9")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"{APP}/tauri.conf.json has '9.9.9'", result.stderr)
        self.assertIn(f"[workspace.package] has '{self.version}'", result.stderr)
        self.assertIn("FAILED (1 of", result.stderr)

    def test_a_missing_tauri_version_fails(self) -> None:
        file = self.root / APP / "tauri.conf.json"
        data = json.loads(file.read_text())
        del data["version"]
        file.write_text(json.dumps(data))
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("tauri.conf.json has '<none>'", result.stderr)

    def test_an_unreadable_changesets_config_fails_instead_of_skipping_files(self) -> None:
        self.edit_json(f"{APP}/tauri.conf.json", version="9.9.9")
        (self.root / ".changeset" / "config.json").write_text("{ not json")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertNotIn("ok", result.stdout)
        self.assertIn("cannot read .changeset/config.json", result.stderr.replace(str(self.root) + "/", ""))

    def test_a_member_outside_crates_is_checked(self) -> None:
        member = self.root / "tools" / "extra"
        (member / "src").mkdir(parents=True)
        (member / "src" / "lib.rs").write_text("")
        (member / "Cargo.toml").write_text('[package]\nname = "extra"\nversion = "9.9.9"\nedition = "2024"\n')
        manifest = self.root / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace("members = [\n", 'members = [\n    "tools/extra",\n', 1))
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("tools/extra/Cargo.toml must say version.workspace = true", result.stderr)
        self.assertIn("Cargo.lock (extra) has '<none>'", result.stderr)

    def test_a_changed_app_manifest_fails(self) -> None:
        file = self.root / APP / "Cargo.toml"
        file.write_text(file.read_text().replace(f'version = "{self.version}"', 'version = "9.9.9"', 1))
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn(f"{APP}/Cargo.toml [package] has '9.9.9'", result.stderr)

    def test_a_stale_lock_entry_fails(self) -> None:
        file = self.root / "Cargo.lock"
        old = f'name = "stepclimb-core"\nversion = "{self.version}"'
        self.assertIn(old, file.read_text())
        file.write_text(file.read_text().replace(old, 'name = "stepclimb-core"\nversion = "9.9.9"'))
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("Cargo.lock (stepclimb-core) has '9.9.9'", result.stderr)

    def test_a_crate_with_its_own_version_fails(self) -> None:
        file = self.root / "crates" / "stepclimb-core" / "Cargo.toml"
        file.write_text(re.sub(r"^version\.workspace = true$", 'version = "9.9.9"', file.read_text(), count=1, flags=re.M))
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("crates/stepclimb-core/Cargo.toml must say version.workspace = true", result.stderr)

    def test_an_app_package_json_with_another_version_fails(self) -> None:
        self.edit_json("web/package.json", version="9.9.9")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("web/package.json has '9.9.9'", result.stderr)

    def test_a_stale_openapi_version_fails(self) -> None:
        file = self.root / "openapi/stepclimb.json"
        data = json.loads(file.read_text())
        data["info"]["version"] = "9.9.9"
        file.write_text(json.dumps(data, indent=2) + "\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("openapi/stepclimb.json info.version has '9.9.9'", result.stderr)

    def test_a_changesets_package_keeps_its_own_version(self) -> None:
        self.edit_json("packages/ui/package.json", version="9.9.9")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_set_version_writes_every_manifest_and_leaves_the_lockfiles(self) -> None:
        self.edit_json("web/package.json", version=self.version)
        before = (self.root / APP / "tauri.conf.json").read_text()
        result = self.run_script("set-version", "0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.read_version(), "0.1.0")
        after = (self.root / APP / "tauri.conf.json").read_text()
        self.assertEqual(after, before.replace(f'"version": "{self.version}"', '"version": "0.1.0"', 1))
        self.assertEqual(json.loads((self.root / "web/package.json").read_text())["version"], "0.1.0")
        openapi = (self.root / "openapi/stepclimb.json").read_text()
        self.assertEqual(json.loads(openapi)["info"]["version"], "0.1.0")
        self.assertEqual(openapi.count('"version": "0.1.0"'), 1)
        self.assertNotEqual(json.loads((self.root / "packages/ui/package.json").read_text())["version"], "0.1.0")
        failures = [line for line in self.check().stderr.splitlines() if " has '" in line]
        self.assertTrue(failures)
        self.assertTrue(all(re.match(r"version-check: (\S+/)?Cargo\.lock ", line) for line in failures), failures)

    def test_set_version_then_refreshed_lockfiles_agree(self) -> None:
        self.assertEqual(self.run_script("set-version", "0.1.0").returncode, 0)
        for lock in ("Cargo.lock", f"{APP}/Cargo.lock"):
            file = self.root / lock
            file.write_text(re.sub(
                rf'(name = "stepclimb[a-z-]*"\nversion = )"{re.escape(self.version)}"', r'\1"0.1.0"',
                file.read_text()))
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("version-check: ok (0.1.0 in ", result.stdout)

    def test_set_version_refuses_what_is_not_a_version(self) -> None:
        for bad in ("v0.1.0", "0.1", "01.0.0", "0.1.0+build", "0.1.0-rc.01", "0.1.0-00", "0.1.0-", ""):
            with self.subTest(bad=bad):
                result = self.run_script("set-version", bad)
                self.assertEqual(result.returncode, 2)
                self.assertIn("is not a version", result.stderr)
        self.assertEqual(self.read_version(), self.version)

    def test_set_version_accepts_a_pre_release(self) -> None:
        for good in ("0.0.1-test.1", "1.0.0-0", "1.0.0-0a.1", "1.0.0-x-y.10"):
            with self.subTest(good=good):
                result = self.run_script("set-version", good)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.read_version(), good)


if __name__ == "__main__":
    unittest.main()
