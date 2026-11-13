# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the format-on-edit hook. Run with `just agents-check`."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import format_on_edit  # noqa: E402

HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROOT = os.path.dirname(os.path.dirname(HOOKS))


def run_hook(payload):
    return subprocess.run(
        ["bash", os.path.join(HOOKS, "format-on-edit.sh")],
        input=json.dumps(payload), capture_output=True, text=True, check=False,
    )


class Choice(unittest.TestCase):
    def test_rust_goes_to_rustfmt(self):
        self.assertEqual(format_on_edit.formatter("/r/a.rs", "/r")[0], "rustfmt")

    def test_web_files_go_to_the_repos_biome(self):
        biome = os.path.join(ROOT, "node_modules", ".bin", "biome")
        self.assertTrue(os.path.isfile(biome), "run `just web-install` first")
        for name in ("a.ts", "a.tsx", "a.json", "a.css", "a.mjs"):
            command = format_on_edit.formatter(os.path.join(ROOT, name), ROOT)
            self.assertEqual(command[:2], [biome, "format"], name)

    def test_other_files_are_left_alone(self):
        for name in ("a.md", "a.toml", "a.sql", "justfile"):
            self.assertIsNone(format_on_edit.formatter(os.path.join(ROOT, name), ROOT), name)

    def test_no_biome_means_no_web_formatting(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(format_on_edit.formatter(os.path.join(tmp, "a.ts"), tmp))


class Hook(unittest.TestCase):
    def test_formats_an_edited_rust_file_and_exits_0(self):
        self.assertIsNotNone(shutil.which("rustfmt"), "rustfmt must be on PATH")
        with tempfile.TemporaryDirectory() as tmp:
            os.mkdir(os.path.join(tmp, ".git"))
            # rustup picks the toolchain from the repo's pin, as in a real checkout.
            shutil.copy(os.path.join(ROOT, "rust-toolchain.toml"), tmp)
            path = os.path.join(tmp, "a.rs")
            with open(path, "w") as f:
                f.write("fn main(){let x=1;}\n")
            result = run_hook({"cwd": tmp, "tool_input": {"file_path": "a.rs"}})
            with open(path) as f:
                text = f.read()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(text, "fn main() {\n    let x = 1;\n}\n")

    def test_never_blocks_on_bad_input(self):
        for raw in ("not json", json.dumps({"tool_input": {"file_path": "/nonexistent/a.rs"}})):
            result = subprocess.run(
                ["bash", os.path.join(HOOKS, "format-on-edit.sh")],
                input=raw, capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 0, raw)


if __name__ == "__main__":
    unittest.main()
