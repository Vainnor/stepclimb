# SPDX-License-Identifier: AGPL-3.0-or-later
"""The release artifacts (bin/release_artifacts.py; docs/specs/m0/01-repo-and-ci.md B16, T9).
The repository tests run in test_release's scratch clones, on a tag merged into a local bare
`origin`'s main. docker, skopeo, cosign and gh are fakes on PATH that log their arguments:
fake docker writes a two-platform OCI archive and SPDX SBOMs, fake skopeo keeps a "registry" in
a directory. No test builds an image, reaches a registry or Sigstore, or creates a release."""

from __future__ import annotations

import hashlib
import io
import json
import os
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import release_artifacts as a  # noqa: E402
import test_release  # noqa: E402
from release import ReleaseError  # noqa: E402

# Writes OCI archives the way `docker buildx build --output type=oci` lays them out: index.json
# naming one image index whose blob lists one image per platform.
OCI = '''
import hashlib, io, json, tarfile
def blob(tar, data):
    digest = hashlib.sha256(data).hexdigest()
    info = tarfile.TarInfo(f"blobs/sha256/{digest}"); info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
    return "sha256:" + digest
def write_archive(path, platforms, salt=""):
    with tarfile.open(path, "w") as tar:
        manifests = []
        for platform in platforms:
            os_, arch = platform.split("/")
            data = json.dumps({"schemaVersion": 2, "image": platform, "salt": salt}).encode()
            manifests.append({"mediaType": "application/vnd.oci.image.manifest.v1+json",
                              "digest": blob(tar, data), "size": len(data),
                              "platform": {"os": os_, "architecture": arch}})
        index = json.dumps({"schemaVersion": 2, "manifests": manifests}).encode()
        layout = json.dumps({"schemaVersion": 2, "manifests": [{
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "digest": blob(tar, index), "size": len(index)}]}).encode()
        info = tarfile.TarInfo("index.json"); info.size = len(layout)
        tar.addfile(info, io.BytesIO(layout))
def index_bytes(path):
    with tarfile.open(path) as tar:
        digest = json.loads(tar.extractfile("index.json").read())["manifests"][0]["digest"]
        return tar.extractfile("blobs/sha256/" + digest.split(":")[1]).read()
'''
LOG = '''
import json, os, sys
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write(json.dumps([os.path.basename(sys.argv[0]), *sys.argv[1:]]) + "\\n")
'''
FAKE_DOCKER = "#!/usr/bin/env python3\n" + OCI + LOG + '''
args = sys.argv[1:]
if args[0] == "version":
    print(os.environ.get("FAKE_ENGINE", "linux/amd64"))
elif args[:2] == ["buildx", "build"]:
    dest = next(a for a in args if a.startswith("type=oci,dest=")).split("dest=", 1)[1]
    write_archive(dest, os.environ.get("FAKE_PLATFORMS", "linux/amd64,linux/arm64").split(","))
elif args[0] == "run":
    platform = args[args.index("--platform") + 1]
    purls = {"deb": "pkg:deb/debian/libc6@2.36-9?arch=" + platform.split("/")[1],
             "cargo": "pkg:cargo/axum@0.8.4",
             "server": os.environ["FAKE_SERVER_PURL"]}
    for drop in os.environ.get("FAKE_SBOM_DROP", "").split(","):
        purls.pop(drop, None)
    packages = [{"name": k, "externalRefs": [{"referenceType": "purl", "referenceLocator": v}]}
                for k, v in purls.items()]
    print(json.dumps({"spdxVersion": os.environ.get("FAKE_SPDX", "SPDX-2.3"), "packages": packages}))
else:
    sys.exit("fake docker: unexpected " + " ".join(args))
'''
FAKE_SKOPEO = "#!/usr/bin/env python3\n" + OCI + LOG + '''
registry = os.environ["FAKE_REGISTRY"]
args = sys.argv[1:]
def stored(ref):
    return os.path.join(registry, ref.replace("/", "_").replace(":", "_"))
if args[:2] == ["inspect", "--raw"]:
    if os.environ.get("FAKE_INSPECT_ERROR"):
        sys.exit("unauthorized: authentication required")
    path = stored(args[2])
    if not os.path.exists(path):
        print("manifest unknown", file=sys.stderr); sys.exit(1)
    sys.stdout.buffer.write(open(path, "rb").read())
elif args[:2] == ["--insecure-policy", "copy"]:
    source, dest = args[-2].split("oci-archive:", 1)[1], args[-1]
    data = index_bytes(source)
    if os.environ.get("FAKE_PUSH_CHANGES_DIGEST"):
        data += b" "
    open(stored(dest), "wb").write(data)
else:
    sys.exit("fake skopeo: unexpected " + " ".join(args))
'''
FAKE_TOOL = "#!/usr/bin/env python3\n" + LOG + '''
fail = os.environ.get("FAKE_FAIL", "")
if fail and " ".join([os.path.basename(sys.argv[0]), *sys.argv[1:3]]).startswith(fail):
    sys.exit(1)
'''


class Tagged(test_release.Scratch):
    """A release tagged on main and checked out in its own worktree, with `verify` run, as
    `just release` leaves it before the artifacts step."""

    TAG = "v0.1.0-test.1"

    def setUp(self) -> None:
        super().setUp()
        self.land("feat(server): add the probe (#1)")
        self.assertEqual(self.prepare(self.TAG[1:]).returncode, 0)
        self.release_into_main(self.TAG[1:])
        self.git(self.work, "fetch", "-q", "origin", f"refs/tags/{self.TAG}:refs/tags/{self.TAG}")
        self.tagged = self.tmp / "tagged"
        self.git(self.work, "worktree", "add", "-q", "--detach", str(self.tagged), self.TAG)
        self.assertEqual(self.run_release("verify", self.TAG, cwd=self.tagged).returncode, 0)
        self.commit_sha = self.git(self.tagged, "rev-parse", "HEAD")
        self.bin = self.tmp / "fakebin"
        self.bin.mkdir()
        for name, text in (("docker", FAKE_DOCKER), ("skopeo", FAKE_SKOPEO),
                           ("cosign", FAKE_TOOL), ("gh", FAKE_TOOL)):
            (self.bin / name).write_text(text)
            (self.bin / name).chmod(0o755)
        self.log = self.tmp / "tools.log"
        self.registry = self.tmp / "registry"
        self.registry.mkdir()
        self.env = {"PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}", "FAKE_LOG": str(self.log),
                    "FAKE_REGISTRY": str(self.registry),
                    "FAKE_SERVER_PURL": f"pkg:cargo/stepclimb-server@{self.TAG[1:]}"}

    def artifacts(self, *args: str, **env: str):
        with mock.patch.dict(os.environ, {**self.env, **env}):
            return test_release.subprocess.run(
                [sys.executable, "-B", str(Path(a.__file__)), *args], cwd=self.tagged,
                capture_output=True, text=True)

    def calls(self, tool: str | None = None) -> list[list[str]]:
        if not self.log.exists():
            return []
        lines = [json.loads(line) for line in self.log.read_text().splitlines()]
        return [line for line in lines if tool is None or line[0] == tool]

    def out(self, name: str) -> Path:
        return self.tagged / "target/release" / f"{self.TAG}-{name}"

    def stamp(self) -> None:
        git_dir = Path(self.git(self.tagged, "rev-parse", "--absolute-git-dir"))
        (git_dir / "stepclimb-ci-pass").write_text(self.commit_sha + "\n")

    def built(self) -> dict:
        result = self.artifacts("build", self.TAG)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.log.unlink()
        return json.loads(self.out("artifacts.json").read_text())

    def assert_build_refused(self, message: str, **env: str) -> None:
        result = self.artifacts("build", self.TAG, **env)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(message, result.stderr)
        self.assertFalse(self.out("artifacts.json").exists())
        self.assertEqual(self.calls("skopeo") + self.calls("cosign") + self.calls("gh"), [])


class Build(Tagged):
    def test_builds_both_platforms_and_an_sbom_each_and_publishes_nothing(self) -> None:
        before = self.origin_refs()
        result = self.artifacts("build", self.TAG)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Nothing was published", result.stdout)
        self.assertIn(f"just release-publish {self.TAG} --dry-run", result.stdout)
        [build] = [c for c in self.calls("docker") if c[1:3] == ["buildx", "build"]]
        self.assertIn("linux/amd64,linux/arm64", build)
        self.assertIn("--provenance=false", build)
        self.assertIn(f"org.opencontainers.image.revision={self.commit_sha}", build)
        self.assertIn(f"org.opencontainers.image.version={self.TAG[1:]}", build)
        scans = [c for c in self.calls("docker") if c[1] == "run"]
        self.assertEqual([c[c.index("--platform") + 1] for c in scans], list(a.PLATFORMS))
        for scan in scans:
            self.assertEqual(scan[scan.index("--network") + 1], "none")
            self.assertIn(a.SYFT_IMAGE, scan)
        manifest = json.loads(self.out("artifacts.json").read_text())
        index, platforms = a.read_index(self.out("image.oci.tar"))
        self.assertEqual((manifest["index"], manifest["platforms"]), (index, platforms))
        self.assertEqual(manifest["commit"], self.commit_sha)
        self.assertEqual(manifest["image"], f"{a.IMAGE}:{self.TAG[1:]}")
        body = self.out("release.md").read_text()
        self.assertTrue(body.startswith(self.out("notes.md").read_text().rstrip("\n")))
        self.assertIn(index, body)
        self.assertIn("### Third-party programs in the image", body)
        self.assertIn("/LICENSES", body)
        self.assertTrue(self.out("sbom-linux-arm64.spdx.json").is_file())
        self.assertEqual(self.calls("skopeo") + self.calls("cosign") + self.calls("gh"), [])
        self.assertEqual(self.origin_refs(), before)

    def test_refuses_an_arm64_engine_before_building(self) -> None:
        self.assert_build_refused("the Docker engine is linux/arm64", FAKE_ENGINE="linux/arm64")
        self.assertEqual([c for c in self.calls("docker") if c[1] == "buildx"], [])

    def test_refuses_an_index_without_both_platforms(self) -> None:
        self.assert_build_refused("holds linux/amd64, not linux/amd64, linux/arm64",
                                  FAKE_PLATFORMS="linux/amd64")
        self.assert_build_refused("holds linux/amd64, linux/arm64, linux/riscv64",
                                  FAKE_PLATFORMS="linux/amd64,linux/arm64,linux/riscv64")

    def test_refuses_an_sbom_without_the_crates(self) -> None:
        self.assert_build_refused("SBOM lists no crates, no pkg:cargo/stepclimb-server@0.1.0-test.1",
                                  FAKE_SBOM_DROP="cargo,server")
        self.assert_build_refused("lists no pkg:cargo/stepclimb-server@0.1.0-test.1",
                                  FAKE_SERVER_PURL="pkg:cargo/stepclimb-server@0.0.9")

    def test_refuses_an_sbom_without_debian_packages_or_not_spdx_2_3(self) -> None:
        self.assert_build_refused("SBOM lists no Debian packages", FAKE_SBOM_DROP="deb")
        self.assert_build_refused("is not SPDX 2.3", FAKE_SPDX="SPDX-2.2")

    def test_refuses_a_dirty_tree_or_a_head_that_is_not_the_tag(self) -> None:
        (self.tagged / "stray.txt").write_text("x")
        self.assert_build_refused("the working tree has changes")
        (self.tagged / "stray.txt").unlink()
        self.git(self.tagged, "checkout", "-q", "--detach", "HEAD^")
        self.assert_build_refused(f"HEAD is not {self.TAG}")
        self.assertEqual(self.calls("docker"), [])

    def test_refuses_a_tag_whose_notes_were_not_written(self) -> None:
        self.out("notes.md").unlink()
        self.assert_build_refused(f"{self.TAG}-notes.md is missing")


class Publish(Tagged):
    def setUp(self) -> None:
        super().setUp()
        self.manifest = self.built()
        self.stamp()

    def test_dry_run_prints_every_step_and_runs_none(self) -> None:
        result = self.artifacts("publish", self.TAG, "--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Nothing was run or published", result.stdout)
        out = result.stdout
        index, amd64 = self.manifest["index"], self.manifest["platforms"]["linux/amd64"]
        order = [f"skopeo --insecure-policy copy --all --preserve-digests oci-archive:{self.out('image.oci.tar')} "
                 f"docker://{a.IMAGE}:{self.TAG[1:]}",
                 f"cosign sign --yes --recursive {a.IMAGE}@{index}",
                 f"cosign attest --yes --type spdxjson --predicate {self.out('sbom-linux-amd64.spdx.json')} "
                 f"{a.IMAGE}@{amd64}",
                 f"gh release create {self.TAG} --verify-tag --title 'Stepclimb {self.TAG}' "
                 f"--notes-file {self.out('release.md')} {self.out('sbom-linux-amd64.spdx.json')} "
                 f"{self.out('sbom-linux-arm64.spdx.json')} --prerelease"]
        positions = [out.index(line) for line in order]
        self.assertEqual(positions, sorted(positions))
        self.assertEqual(self.calls(), [])

    def test_publishes_in_order_signing_only_the_pushed_digests(self) -> None:
        result = self.artifacts("publish", self.TAG)
        self.assertEqual(result.returncode, 0, result.stderr)
        tools = [c[0] + " " + c[1] for c in self.calls()]
        self.assertEqual(tools, ["skopeo inspect", "skopeo --insecure-policy", "skopeo inspect", "cosign sign",
                                 "cosign attest", "cosign attest", "gh release"])
        signed = self.calls("cosign")
        self.assertEqual(signed[0][-1], f"{a.IMAGE}@{self.manifest['index']}")
        self.assertEqual([c[-1] for c in signed[1:]],
                         [f"{a.IMAGE}@{self.manifest['platforms'][p]}" for p in a.PLATFORMS])
        self.assertEqual([c[c.index("--predicate") + 1] for c in signed[1:]],
                         [str(self.out(f"sbom-{p.replace('/', '-')}.spdx.json")) for p in a.PLATFORMS])
        [gh] = self.calls("gh")
        self.assertEqual(gh[-1], "--prerelease")

    def test_refuses_without_a_pass_on_the_tag(self) -> None:
        git_dir = Path(self.git(self.tagged, "rev-parse", "--absolute-git-dir"))
        (git_dir / "stepclimb-ci-pass").write_text("0" * 40 + "\n")
        for args in (("--dry-run",), ()):
            result = self.artifacts("publish", self.TAG, *args)
            self.assertEqual(result.returncode, 1)
            self.assertIn("just ci has not passed", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_refuses_an_artifact_changed_after_the_build(self) -> None:
        sbom = self.out("sbom-linux-arm64.spdx.json")
        sbom.write_text(sbom.read_text().replace("libc6", "libc7"))
        result = self.artifacts("publish", self.TAG)
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing or changed since it was built", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_refuses_an_archive_rebuilt_after_the_manifest(self) -> None:
        namespace: dict = {}
        exec(OCI, namespace)
        namespace["write_archive"](str(self.out("image.oci.tar")), a.PLATFORMS, salt="rebuilt")
        result = self.artifacts("publish", self.TAG, "--dry-run")
        self.assertEqual(result.returncode, 1)
        self.assertIn("image.oci.tar is missing or changed", result.stderr)

    def test_refuses_a_dirty_tree(self) -> None:
        (self.tagged / "stray.txt").write_text("x")
        result = self.artifacts("publish", self.TAG, "--dry-run")
        self.assertEqual(result.returncode, 1)
        self.assertIn("the working tree has changes", result.stderr)

    def edit_manifest(self, **changes: object) -> None:
        path = self.out("artifacts.json")
        path.write_text(json.dumps({**json.loads(path.read_text()), **changes}))

    def test_refuses_a_manifest_whose_digests_differ_from_the_archive(self) -> None:
        self.edit_manifest(index="sha256:" + "1" * 64)
        result = self.artifacts("publish", self.TAG, "--dry-run")
        self.assertEqual(result.returncode, 1)
        self.assertIn("the image archive's digests differ", result.stderr)

    def test_refuses_a_manifest_naming_another_image(self) -> None:
        self.edit_manifest(image="ghcr.io/someone-else/stepclimb:0.1.0-test.1")
        result = self.artifacts("publish", self.TAG, "--dry-run")
        self.assertEqual(result.returncode, 1)
        self.assertIn("names the image ghcr.io/someone-else/stepclimb", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_never_replaces_a_published_version(self) -> None:
        (self.registry / f"docker://{a.IMAGE}:{self.TAG[1:]}".replace("/", "_").replace(":", "_")).write_text("{}")
        result = self.artifacts("publish", self.TAG)
        self.assertEqual(result.returncode, 1)
        self.assertIn("a published version is never replaced", result.stderr)
        self.assertEqual([c[1] for c in self.calls()], ["inspect"])

    def test_a_second_run_after_a_failed_step_skips_the_push(self) -> None:
        result = self.artifacts("publish", self.TAG, FAKE_FAIL="cosign attest")
        self.assertEqual(result.returncode, 1)
        self.assertIn("Steps before it are done", result.stderr)
        self.assertIn(f"gh release create {self.TAG}", result.stderr)
        self.assertEqual(self.calls("gh"), [])
        self.log.unlink()
        result = self.artifacts("publish", self.TAG)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("is already this build; not pushed again", result.stdout)
        self.assertEqual([c[1] for c in self.calls("skopeo")], ["inspect"])
        self.assertEqual(len(self.calls("gh")), 1)

    def test_signs_nothing_when_the_registry_serves_another_digest(self) -> None:
        result = self.artifacts("publish", self.TAG, FAKE_PUSH_CHANGES_DIGEST="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("nothing was signed", result.stderr)
        self.assertEqual(self.calls("cosign") + self.calls("gh"), [])

    def test_a_registry_error_stops_before_the_push(self) -> None:
        result = self.artifacts("publish", self.TAG, FAKE_INSPECT_ERROR="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("unauthorized", result.stderr)
        self.assertEqual([c[1] for c in self.calls()], ["inspect"])

    def test_a_release_version_is_not_a_prerelease(self) -> None:
        steps = a.publish_commands(self.tagged, "v0.1.0", {**self.manifest})
        self.assertNotIn("--prerelease", steps[-1][1])
        self.assertEqual(steps[0][1][-1], f"docker://{a.IMAGE}:0.1.0")

    def test_refuses_when_a_tool_is_missing(self) -> None:
        with mock.patch.object(a.shutil, "which", side_effect=lambda t: None if t == "cosign" else t), \
                mock.patch.object(a.release, "require_clean"), \
                mock.patch.object(a.release, "tag_commit", return_value=self.commit_sha), \
                mock.patch.object(a.release, "require_ci_pass"):
            with self.assertRaisesRegex(ReleaseError, "cosign not on PATH"):
                a.publish(self.tagged, self.TAG, dry_run=True)


class Archive(unittest.TestCase):
    def write(self, layout: dict, blobs: dict[str, bytes]) -> Path:
        path = Path(tempfile.mkdtemp(prefix="oci-")) / "image.tar"
        self.addCleanup(lambda: __import__("shutil").rmtree(path.parent))
        with tarfile.open(path, "w") as tar:
            for name, data in {"index.json": json.dumps(layout).encode(), **blobs}.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
        return path

    def test_refuses_a_blob_that_does_not_match_its_digest(self) -> None:
        digest = "sha256:" + hashlib.sha256(b"original").hexdigest()
        layout = {"manifests": [{"mediaType": a.INDEX_TYPES[0], "digest": digest}]}
        path = self.write(layout, {f"blobs/sha256/{digest[7:]}": b"tampered"})
        with self.assertRaisesRegex(ReleaseError, "does not match its digest"):
            a.read_index(path)

    def test_refuses_an_index_listing_a_platform_twice(self) -> None:
        manifest = {"digest": "sha256:" + "2" * 64, "platform": {"os": "linux", "architecture": "amd64"}}
        index = json.dumps({"manifests": [manifest, manifest]}).encode()
        digest = "sha256:" + hashlib.sha256(index).hexdigest()
        layout = {"manifests": [{"mediaType": a.INDEX_TYPES[0], "digest": digest}]}
        with self.assertRaisesRegex(ReleaseError, "lists linux/amd64 twice"):
            a.read_index(self.write(layout, {f"blobs/sha256/{digest[7:]}": index}))

    def test_refuses_a_layout_that_is_not_one_index(self) -> None:
        layout = {"manifests": [{"mediaType": "application/vnd.oci.image.manifest.v1+json",
                                 "digest": "sha256:" + "0" * 64}]}
        with self.assertRaisesRegex(ReleaseError, "exactly one image index"):
            a.read_index(self.write(layout, {}))


if __name__ == "__main__":
    unittest.main()
