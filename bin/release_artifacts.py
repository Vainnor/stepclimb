# SPDX-License-Identifier: AGPL-3.0-or-later
"""The release artifacts (docs/specs/m0/01-repo-and-ci.md B16, Q1;
docs/adr/0031-release-image-signing-and-sbom.md).

`build <vX.Y.Z>` is the last step of `just release`, after `release.py finish`. In a clean
worktree of the annotated tag, on a Docker engine for linux/amd64, it builds deploy/Dockerfile for
linux/amd64 and linux/arm64 into one OCI archive, has syft (a container image pinned by digest,
run without a network) write an SPDX 2.3 SBOM for each platform, checks that each SBOM lists the
Debian packages and the crates in the image, writes the GitHub Release body, and records the
digests and file hashes in target/release/<tag>-artifacts.json. It contacts no registry but
Docker Hub's, for syft's and the Dockerfile's pinned images, and publishes nothing.

`publish <vX.Y.Z> [--dry-run]` (`just release-publish`) is the only step that publishes, and only
when a person runs it. It checks the tag, the `just ci` pass on it and every recorded artifact
against the files, then pushes the archive to GHCR with skopeo (digests kept), checks the
registry's digest, signs the image index and each platform's image with cosign keyless, attaches
each SBOM as a signed cosign attestation, and creates the GitHub Release with the SBOMs.
`--dry-run` makes the same checks and prints those commands without running any.

Usage: release_artifacts.py build <vX.Y.Z> | release_artifacts.py publish <vX.Y.Z> [--dry-run]"""

from __future__ import annotations

import hashlib
import json
import shlex
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import release  # noqa: E402
from release import ReleaseError  # noqa: E402

IMAGE = "ghcr.io/vainnor/stepclimb"
SOURCE = "https://github.com/Vainnor/stepclimb"
PLATFORMS = ("linux/amd64", "linux/arm64")
# anchore/syft v1.55.0 (built 19 Oct 2026), its multi-platform index. Moved by hand, at least
# 14 days after the build it names (docs/adr/0031). Split in two only for the line length: the
# tag and the digest form one reference.
SYFT_IMAGE = ("docker.io/anchore/syft:v1.55.0"
              "@sha256:4cc92b64ca74ffca4c94d4c8fcc01f217731375300be14d0b6eeb2e82eaa110a")
INDEX_TYPES = ("application/vnd.oci.image.index.v1+json",
               "application/vnd.docker.distribution.manifest.list.v2+json")
PUBLISH_TOOLS = ("skopeo", "cosign", "gh")


def out_dir(root: Path) -> Path:
    return root / release.OUT


def paths(root: Path, tag: str) -> dict[str, Path]:
    out = out_dir(root)
    found = {"archive": out / f"{tag}-image.oci.tar", "manifest": out / f"{tag}-artifacts.json",
             "notes": out / f"{tag}-notes.md", "body": out / f"{tag}-release.md"}
    for platform in PLATFORMS:
        found[platform] = out / f"{tag}-sbom-{platform.replace('/', '-')}.spdx.json"
    return found


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def run(command: list[str], what: str, capture: bool = False) -> str:
    """Runs `command`; a missing program or a failure is a ReleaseError naming `what`."""
    try:
        result = subprocess.run(command, capture_output=capture, text=True)
    except FileNotFoundError as error:
        raise ReleaseError(f"{command[0]} is not installed; run inside `nix develop`") from error
    if result.returncode != 0:
        detail = f": {result.stderr.strip()}" if capture and result.stderr else ""
        raise ReleaseError(f"{what} failed (exit {result.returncode}){detail}")
    return result.stdout if capture else ""


def require_amd64_engine() -> None:
    native = run(["docker", "version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"],
                 "docker version", capture=True).strip()
    if native != "linux/amd64":
        raise ReleaseError(f"the Docker engine is {native}; deploy/Dockerfile builds linux/arm64 "
                           "by cross-compiling from linux/amd64, so build the release on an amd64 "
                           "machine or VM")


def build_command(archive: Path, version: str, commit: str) -> list[str]:
    labels = {"org.opencontainers.image.title": "Stepclimb",
              "org.opencontainers.image.source": SOURCE,
              "org.opencontainers.image.version": version,
              "org.opencontainers.image.revision": commit,
              "org.opencontainers.image.licenses": "AGPL-3.0-or-later"}
    command = ["docker", "buildx", "build", "--platform", ",".join(PLATFORMS),
               "--provenance=false", "--sbom=false", "-f", "deploy/Dockerfile",
               "--output", f"type=oci,dest={archive}"]
    for key, value in labels.items():
        command += ["--label", f"{key}={value}"]
    return command + ["."]


def read_blob(archive: tarfile.TarFile, digest: str) -> bytes:
    """The blob `digest` names, refused unless its content hashes to that digest."""
    algorithm, _, hexdigest = digest.partition(":")
    if algorithm != "sha256" or len(hexdigest) != 64:
        raise ReleaseError(f"the image archive names a digest that is not sha256: {digest}")
    try:
        member = archive.extractfile(f"blobs/sha256/{hexdigest}")
    except KeyError as error:
        raise ReleaseError(f"the image archive has no blob {digest}") from error
    data = member.read() if member else b""
    if hashlib.sha256(data).hexdigest() != hexdigest:
        raise ReleaseError(f"the image archive's blob {digest} does not match its digest")
    return data


def read_index(path: Path) -> tuple[str, dict[str, str]]:
    """The image index digest in the OCI archive at `path` and each platform's image digest;
    refused unless the index holds exactly linux/amd64 and linux/arm64."""
    try:
        with tarfile.open(path) as archive:
            layout = json.loads(archive.extractfile("index.json").read())  # type: ignore[union-attr]
            entries = layout.get("manifests", [])
            if len(entries) != 1 or entries[0].get("mediaType") not in INDEX_TYPES:
                raise ReleaseError("the image archive does not hold exactly one image index")
            digest = entries[0]["digest"]
            index = json.loads(read_blob(archive, digest))
    except (OSError, KeyError, ValueError, tarfile.TarError) as error:
        raise ReleaseError(f"cannot read the image archive {path}: {error}") from error
    found: dict[str, str] = {}
    for manifest in index.get("manifests", []):
        platform = manifest.get("platform", {})
        name = f"{platform.get('os')}/{platform.get('architecture')}"
        if name in found:
            raise ReleaseError(f"the image index lists {name} twice")
        found[name] = manifest.get("digest", "")
    if sorted(found) != sorted(PLATFORMS):
        raise ReleaseError(f"the image index holds {', '.join(sorted(found)) or 'nothing'}, "
                           f"not {', '.join(PLATFORMS)}")
    return digest, found


def syft_command(archive: Path, platform: str) -> list[str]:
    return ["docker", "run", "--rm", "--network", "none", "-e", "SYFT_CHECK_FOR_APP_UPDATE=false",
            "-v", f"{archive.parent.resolve()}:/release:ro", SYFT_IMAGE, "scan",
            f"oci-archive:/release/{archive.name}", "--platform", platform, "-o", "spdx-json"]


def check_sbom(text: str, platform: str, version: str) -> None:
    """Refused unless it is SPDX 2.3 JSON naming Debian packages, crates and the server crate at
    the release's version."""
    try:
        sbom = json.loads(text)
    except ValueError as error:
        raise ReleaseError(f"syft wrote no JSON for {platform}: {error}") from error
    if sbom.get("spdxVersion") != "SPDX-2.3":
        raise ReleaseError(f"the {platform} SBOM is not SPDX 2.3 ({sbom.get('spdxVersion')})")
    purls = {ref.get("referenceLocator", "") for package in sbom.get("packages", [])
             for ref in package.get("externalRefs", []) if ref.get("referenceType") == "purl"}
    server = f"pkg:cargo/stepclimb-server@{version}"
    missing = [what for what, ok in (
        ("Debian packages", any(p.startswith("pkg:deb/") for p in purls)),
        ("crates", any(p.startswith("pkg:cargo/") for p in purls)),
        (server, server in purls)) if not ok]
    if missing:
        raise ReleaseError(f"the {platform} SBOM lists no {', no '.join(missing)}; the server "
                           "binary must carry cargo-auditable's dependency list (deploy/Dockerfile)")


def release_body(notes: str, index: str, platforms: dict[str, str], version: str) -> str:
    lines = [notes.rstrip("\n"), "", "### Container image", "",
             f"`{IMAGE}:{version}`, image index `{index}`:", ""]
    lines += [f"- {platform}: `{platforms[platform]}`" for platform in PLATFORMS]
    lines += ["", "Signed with cosign (keyless); each platform's SPDX SBOM is attached below and "
              "as a signed cosign attestation (`--type spdxjson`).", ""]
    return "\n".join(lines)


def prepare_build(root: Path, tag: str) -> tuple[str, str, dict[str, Path]]:
    """The checks before building: returns the version, the tag's commit and the output paths,
    with any earlier output for the tag removed."""
    version = release.tag_version(tag)
    release.require_clean(root)
    commit = release.tag_commit(root, tag)
    files = paths(root, tag)
    if not files["notes"].is_file():
        raise ReleaseError(f"{files['notes']} is missing; run just release {tag}")
    require_amd64_engine()
    for key, path in files.items():
        if key != "notes":
            path.unlink(missing_ok=True)
    return version, commit, files


def build(root: Path, tag: str) -> int:
    version, commit, files = prepare_build(root, tag)
    archive = files["archive"]
    run(build_command(archive, version, commit), "the image build")
    index, platforms = read_index(archive)
    sboms = {}
    for platform in PLATFORMS:
        text = run(syft_command(archive, platform), f"syft on {platform}", capture=True)
        check_sbom(text, platform, version)
        files[platform].write_text(text)
        sboms[platform] = {"file": files[platform].name, "sha256": sha256_file(files[platform])}
    files["body"].write_text(release_body(files["notes"].read_text(), index, platforms, version))
    manifest = {"tag": tag, "commit": commit, "image": f"{IMAGE}:{version}", "index": index,
                "platforms": platforms, "sboms": sboms,
                "archive": {"file": archive.name, "sha256": sha256_file(archive)},
                "body": {"file": files["body"].name, "sha256": sha256_file(files["body"])}}
    files["manifest"].write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"""release: built {IMAGE}:{version} for {', '.join(PLATFORMS)} from {tag} ({commit[:12]}).
Image index {index}; archive, SBOMs and release body in {release.OUT}. Nothing was published.
Rehearse, then publish (pushes to GHCR, signs with cosign keyless, creates the GitHub Release):

  just release-publish {tag} --dry-run
  just release-publish {tag}""")
    return 0


def load_manifest(root: Path, tag: str, commit: str) -> dict:
    """artifacts.json, refused unless it names this tag and commit and every file it records
    still hashes to what `build` wrote."""
    files = paths(root, tag)
    try:
        manifest = json.loads(files["manifest"].read_text())
    except (OSError, ValueError) as error:
        raise ReleaseError(f"cannot read {files['manifest']} ({error}); run just release {tag}") from error
    if manifest.get("tag") != tag or manifest.get("commit") != commit:
        raise ReleaseError(f"{files['manifest']} was built from another tag or commit; run just release {tag}")
    if manifest.get("image") != f"{IMAGE}:{release.tag_version(tag)}":
        raise ReleaseError(f"{files['manifest']} names the image {manifest.get('image')}, not "
                           f"{IMAGE}:{release.tag_version(tag)}; run just release {tag}")
    recorded = [(manifest["archive"], files["archive"]), (manifest["body"], files["body"])]
    recorded += [(manifest["sboms"][p], files[p]) for p in PLATFORMS]
    for entry, path in recorded:
        if not path.is_file() or entry.get("sha256") != sha256_file(path):
            raise ReleaseError(f"{path} is missing or changed since it was built; run just release {tag}")
    index, platforms = read_index(files["archive"])
    if (index, platforms) != (manifest.get("index"), manifest.get("platforms")):
        raise ReleaseError(f"the image archive's digests differ from {files['manifest']}")
    return manifest


def publish_commands(root: Path, tag: str, manifest: dict) -> list[tuple[str, list[str]]]:
    """Each publishing step, in order: what it does and its command."""
    files = paths(root, tag)
    version = release.tag_version(tag)
    pinned = f"{IMAGE}@{manifest['index']}"
    # skopeo refuses to copy without a containers policy.json, which most machines lack. Its
    # policy decides which source images to trust; the source here is the archive `build` wrote
    # and load_manifest checked, so the check is skipped for this copy.
    steps = [("push the image", ["skopeo", "--insecure-policy", "copy", "--all", "--preserve-digests",
                                 f"oci-archive:{files['archive']}", f"docker://{IMAGE}:{version}"]),
             ("sign the image index and each platform's image",
              ["cosign", "sign", "--yes", "--recursive", pinned])]
    for platform in PLATFORMS:
        steps.append((f"attach the {platform} SBOM",
                      ["cosign", "attest", "--yes", "--type", "spdxjson", "--predicate",
                       str(files[platform]), f"{IMAGE}@{manifest['platforms'][platform]}"]))
    gh = ["gh", "release", "create", tag, "--verify-tag", "--title", f"Stepclimb {tag}",
          "--notes-file", str(files["body"]), *(str(files[p]) for p in PLATFORMS)]
    if "-" in version:
        gh.append("--prerelease")
    steps.append(("create the GitHub Release", gh))
    return steps


def registry_digest(reference: str) -> str | None:
    """The digest the registry serves for `reference`, or None when it has no such manifest."""
    try:
        result = subprocess.run(["skopeo", "inspect", "--raw", f"docker://{reference}"],
                                capture_output=True)
    except FileNotFoundError as error:
        raise ReleaseError("skopeo is not installed; run inside `nix develop`") from error
    if result.returncode == 0:
        return "sha256:" + hashlib.sha256(result.stdout).hexdigest()
    error = result.stderr.decode(errors="replace")
    if "manifest unknown" in error.lower():
        return None
    raise ReleaseError(f"skopeo inspect {reference} failed: {error.strip()}")


def require_unpublished_or_same(manifest: dict) -> bool:
    """True when the registry already serves this exact index under the version's tag (a
    publish run again after a later step failed); refused when it serves another image."""
    served = registry_digest(manifest["image"])
    if served is not None and served != manifest["index"]:
        raise ReleaseError(f"{manifest['image']} already exists as {served}, not this build's "
                           f"{manifest['index']}; a published version is never replaced")
    return served is not None


def run_publish(steps: list[tuple[str, list[str]]], manifest: dict) -> None:
    already = require_unpublished_or_same(manifest)
    for number, (what, command) in enumerate(steps):
        if number == 0 and already:
            print(f"release-publish: {manifest['image']} is already this build; not pushed again")
            continue
        print(f"release-publish: {what}: {shlex.join(command)}", flush=True)
        try:
            run(command, what)
        except ReleaseError as error:
            rest = "\n".join(f"  {shlex.join(c)}" for _, c in steps[number:])
            raise ReleaseError(f"{error}. Steps before it are done; run again, or by hand:\n{rest}") from error
        if number == 0 and registry_digest(manifest["image"]) != manifest["index"]:
            raise ReleaseError(f"the registry serves {manifest['image']} under another digest than "
                               f"{manifest['index']}; nothing was signed")


def publish(root: Path, tag: str, dry_run: bool) -> int:
    release.tag_version(tag)
    release.require_clean(root)
    commit = release.tag_commit(root, tag)
    release.require_ci_pass(root, commit)
    manifest = load_manifest(root, tag, commit)
    missing = [tool for tool in PUBLISH_TOOLS if shutil.which(tool) is None]
    if missing:
        raise ReleaseError(f"{', '.join(missing)} not on PATH; run inside `nix develop`")
    steps = publish_commands(root, tag, manifest)
    if dry_run:
        commands = "\n".join(f"  # {what}\n  {shlex.join(command)}" for what, command in steps)
        print(f"release-publish: dry run for {tag}; the artifacts match {release.OUT / (tag + '-artifacts.json')}.\n"
              f"Nothing was run or published. A real run first checks that {manifest['image']} is "
              f"absent or already {manifest['index']}, then runs:\n\n{commands}")
        return 0
    run_publish(steps, manifest)
    print(f"release-publish: published {manifest['image']} ({manifest['index']}) and the {tag} GitHub Release")
    return 0


def main(argv: list[str]) -> int:
    root = Path.cwd()
    try:
        if len(argv) == 2 and argv[0] == "build":
            return build(root, argv[1])
        if argv[:1] == ["publish"] and len(argv) in (2, 3) and argv[2:] in ([], ["--dry-run"]):
            return publish(root, argv[1], dry_run=len(argv) == 3)
        print(__doc__.rsplit("Usage: ", 1)[1].strip(), file=sys.stderr)
        return 2
    except ReleaseError as error:
        print(f"release: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
