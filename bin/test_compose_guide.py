# SPDX-License-Identifier: AGPL-3.0-or-later
"""The self-hosting guide's Compose variables table matches deploy/compose/compose.production.yaml
and the Caddyfile: the same `SC_*` names, and each default as the file gives it
(docs/specs/m0/14-compose-deploy.md B22)."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

COMPOSE = Path(__file__).resolve().parent.parent / "deploy" / "compose"

# `${SC_NAME:-default}` (an empty default is none) and `${SC_NAME:?message}` (required).
COMPOSE_VAR = re.compile(r"\$\{(SC_[A-Z_]+)(?::([-?])([^}]*))?\}")
# `{$SC_NAME}` and `{$SC_NAME:default}` in the Caddyfile.
CADDY_VAR = re.compile(r"\{\$(SC_[A-Z_]+)(?::([^}]*))?\}")
ROW = re.compile(r"^\| `(SC_[A-Z_]+)` \| ([^|]+?) \|")


def compose_defaults(text: str) -> dict[str, str]:
    """Each variable's default cell as the guide writes it; a variable used twice must agree."""
    defaults: dict[str, str] = {}
    for name, kind, value in COMPOSE_VAR.findall(text):
        if kind == "?":
            cell = "required"
        elif kind == "-" and value:
            cell = f"`{value}`"
        else:
            cell = "none"
        if defaults.setdefault(name, cell) != cell:
            raise AssertionError(f"{name} has two defaults: {defaults[name]} and {cell}")
    return defaults


def guide_rows(text: str) -> dict[str, str]:
    rows: dict[str, str] = {}
    for line in text.splitlines():
        match = ROW.match(line)
        if match:
            if match.group(1) in rows:
                raise AssertionError(f"{match.group(1)} is in the table twice")
            rows[match.group(1)] = match.group(2)
    return rows


class ComposeVariablesTableTest(unittest.TestCase):
    def setUp(self) -> None:
        self.rows = guide_rows((COMPOSE / "README.md").read_text(encoding="utf-8"))
        production = (COMPOSE / "compose.production.yaml").read_text(encoding="utf-8")
        self.defaults = compose_defaults(production)

    def test_the_table_names_every_variable_with_its_default(self) -> None:
        self.assertEqual(self.rows, self.defaults)

    def test_the_caddyfile_uses_only_documented_variables_with_the_same_defaults(self) -> None:
        caddyfile = (COMPOSE / "Caddyfile").read_text(encoding="utf-8")
        for name, default in CADDY_VAR.findall(caddyfile):
            self.assertIn(name, self.rows, f"{name} is not in the guide's table")
            if default:
                self.assertEqual(self.rows[name], f"`{default}`", name)


if __name__ == "__main__":
    unittest.main()
