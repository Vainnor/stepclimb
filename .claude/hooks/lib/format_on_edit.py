# SPDX-License-Identifier: AGPL-3.0-or-later
"""PostToolUse(Write|Edit|MultiEdit): format an edited file with the repo's formatter.

`.rs` goes through rustfmt with the pinned toolchain (rust-toolchain.toml) and rustfmt.toml;
TypeScript, JavaScript, JSON and CSS go through the repo's own Biome (`node_modules/.bin/biome`,
biome.json), so the edit matches what `just web-lint` checks. A file Biome's config excludes
(generated files) is left alone by Biome itself. Never blocks.
"""

import os
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402

BIOME_EXTENSIONS = {".ts", ".tsx", ".js", ".mjs", ".cjs", ".json", ".jsonc", ".css"}


def formatter(path, root):
    """The command that formats `path` in the repo at `root`, or None."""
    ext = os.path.splitext(path)[1].lower()
    if ext == ".rs":
        return ["rustfmt", "--edition", "2024", path]
    if ext in BIOME_EXTENSIONS:
        biome = os.path.join(root, "node_modules", ".bin", "biome")
        if os.path.isfile(biome):
            return [biome, "format", "--write", "--no-errors-on-unmatched", path]
    return None


def main():
    try:
        data = hookio.read_input()
    except ValueError:
        return
    path = data.get("tool_input", {}).get("file_path") or ""
    if not path:
        return
    if not os.path.isabs(path):
        path = os.path.join(data.get("cwd") or os.getcwd(), path)
    root = hookio.repo_root(path)
    if root is None or not os.path.isfile(path):
        return
    command = formatter(path, root)
    if command is None:
        return
    try:
        # From the repo root, so rust-toolchain.toml picks the pinned rustfmt and the repo's
        # rustfmt.toml and biome.json apply.
        subprocess.run(command, cwd=root, capture_output=True, timeout=30, check=False)
    except (OSError, subprocess.TimeoutExpired):
        pass


if __name__ == "__main__":
    try:
        main()
    finally:
        sys.exit(0)
