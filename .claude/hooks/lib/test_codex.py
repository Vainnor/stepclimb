# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the Codex side of the agent setup (docs/agents.md). Run with `just agents-check`.

Covers the apply_patch adapter, the SessionStart instructions printer, the Codex agent and skill
files that point back at .claude/, and the limits codex-path-rules puts on .claude/rules/.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import codex_adapter  # noqa: E402
import codex_instructions  # noqa: E402

LIB = os.path.dirname(os.path.abspath(__file__))
HOOKS = os.path.dirname(LIB)
ROOT = os.path.dirname(os.path.dirname(HOOKS))
# codex-path-rules 0.5.0 src/render.rs: MAX_RULE_CHARS. A longer rule body is truncated.
PATH_RULES_MAX_RULE_CHARS = 6000


def front_matter(path):
    """`key: value` pairs of a Markdown file's leading front matter block."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    match = re.match(r"---\n(.*?)\n---\n", text, re.S)
    return dict(re.findall(r"^([\w-]+): (.*)$", match.group(1), re.M)) if match else {}


def patch(*lines):
    return "\n".join(("*** Begin Patch",) + lines + ("*** End Patch",))


def run_adapter(hook, payload):
    result = subprocess.run(
        ["bash", os.path.join(HOOKS, "codex-adapter.sh"), hook],
        input=json.dumps(payload), capture_output=True, text=True, cwd=ROOT, check=False,
    )
    return result.returncode, result.stderr


def bash_payload(command):
    return {
        "session_id": "test", "cwd": ROOT, "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_use_id": "t1", "tool_input": {"command": command},
    }


def apply_patch_payload(command):
    return {
        "session_id": "test", "cwd": ROOT, "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch", "tool_use_id": "t1", "tool_input": {"command": command},
    }


class PatchPaths(unittest.TestCase):
    def test_reads_every_file_header_once(self):
        body = patch(
            "*** Add File: docs/new.md", "+text",
            "*** Update File: src/a.rs", "*** Move to: src/b.rs", "@@", "-x", "+y",
            "*** Delete File: old.txt",
            "*** Update File: src/a.rs",
        )
        self.assertEqual(
            codex_adapter.patch_paths(body), ["docs/new.md", "src/a.rs", "src/b.rs", "old.txt"],
        )

    def test_reads_indented_and_crlf_headers_as_codex_does(self):
        # Codex trims each patch line before matching a header (apply-patch streaming_parser.rs).
        body = patch("  *** Add File: a.md", "+x", "\t*** Delete File: b.md", "*** Move to: c.md\r")
        self.assertEqual(codex_adapter.patch_paths(body), ["a.md", "b.md", "c.md"])


class ContentGuardThroughAdapter(unittest.TestCase):
    def assert_blocked(self, payload, needle="content-guard: blocked"):
        code, err = run_adapter("content-guard.sh", payload)
        self.assertEqual(code, 2, err)
        self.assertIn(needle, err)

    def test_blocks_a_reference_file(self):
        self.assert_blocked(apply_patch_payload(patch("*** Add File: references/x.pdf", "+x")))

    def test_blocks_a_move_into_private(self):
        body = patch("*** Update File: docs/a.csv", "*** Move to: private/a.csv")
        self.assert_blocked(apply_patch_payload(body), "private/a.csv")

    def test_blocks_indented_headers(self):
        for indent in ("   ", "\t"):
            body = patch(indent + "*** Add File: references/x.pdf", "+x")
            self.assert_blocked(apply_patch_payload(body))

    def test_blocks_every_shell_patch(self):
        # exec_command applies a shell patch in its workdir and any leading cd; on Codex 0.165 the
        # hook cwd is the session cwd and the payload has no workdir, so where the patch lands is
        # unknown. Harmless paths are blocked too.
        body = patch("*** Add File: docs/ok.md", "+x")
        commands = (
            "apply_patch <<'EOF'\n" + body + "\nEOF",
            "applypatch <<'EOF'\n" + body + "\nEOF",
            "cd references && apply_patch <<'EOF'\n" + body + "\nEOF",
            "cd docs && apply_patch <<'EOF'\n" + body + "\nEOF",
            "apply_patch '" + body + "'",
            "echo hi && apply_patch <<'EOF'\n" + body + "\nEOF",
            "cat > p.txt <<'EOF'\n" + body + "\nEOF",
            "printf '%s' '*** Begin Patch'",
            "  *** Add File: x.md",
        )
        for command in commands:
            self.assert_blocked(bash_payload(command), "use the apply_patch tool")

    def test_blocks_a_shell_patch_run_with_a_workdir(self):
        # Codex runs this in references/, and on 0.165 the hook only sees ROOT as its cwd.
        command = "apply_patch <<'EOF'\n" + patch("*** Add File: x.pdf", "+x") + "\nEOF"
        payload = bash_payload(command)
        payload["tool_input"]["workdir"] = "references"
        self.assert_blocked(payload, "use the apply_patch tool")

    def test_allows_the_reference_index(self):
        code, err = run_adapter(
            "content-guard.sh",
            apply_patch_payload(patch("*** Update File: references/INDEX.md", "@@", "+row")),
        )
        self.assertEqual(code, 0, err)

    def test_passes_shell_commands_that_are_not_patches(self):
        for command in (
            "cat references/INDEX.md",
            "rg apply_patch docs",
            "git log --grep applypatch",
            "echo '*** Add File: references/x.pdf' # apply_patch docs",
            "apply_patch <<'EOF'\ngarbage\nEOF",
        ):
            code, err = run_adapter("content-guard.sh", bash_payload(command))
            self.assertEqual(code, 0, f"{command}: {err}")

    def test_blocks_an_apply_patch_body_with_no_file_header(self):
        self.assert_blocked(apply_patch_payload("garbage"), "names no file")

    def test_blocks_malformed_input(self):
        for raw in ("not json", json.dumps(dict(apply_patch_payload(""), tool_input="x"))):
            result = subprocess.run(
                ["bash", os.path.join(HOOKS, "codex-adapter.sh"), "content-guard.sh"],
                input=raw, capture_output=True, text=True, cwd=ROOT, check=False,
            )
            self.assertEqual(result.returncode, 2, raw)
            self.assertIn("blocked", result.stderr)

    def test_blocks_when_the_adapter_itself_crashes(self):
        # With no bash on PATH, running the wrapped hook raises FileNotFoundError; run_guard must
        # turn that into exit 2.
        result = subprocess.run(
            [sys.executable, "-B", os.path.join(LIB, "codex_adapter.py"),
             os.path.join(HOOKS, "content-guard.sh")],
            input=json.dumps(apply_patch_payload(patch("*** Add File: docs/x.md", "+x"))),
            capture_output=True, text=True, cwd=ROOT, env={"PATH": ""}, check=False,
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("codex-adapter: blocked; the hook failed", result.stderr)

    def test_blocks_when_the_wrapped_hook_fails_with_any_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            script = os.path.join(tmp, "crash.sh")
            with open(script, "w") as f:
                f.write("exit 1\n")
            result = subprocess.run(
                [sys.executable, "-B", os.path.join(LIB, "codex_adapter.py"), script],
                input=json.dumps(apply_patch_payload(patch("*** Add File: docs/x.md", "+x"))),
                capture_output=True, text=True, cwd=ROOT, check=False,
            )
        self.assertEqual(result.returncode, 2)
        self.assertIn("crash.sh exited 1", result.stderr)


class CodexInstructions(unittest.TestCase):
    """The Codex SessionStart hook prints CLAUDE.md and the rules Claude Code loads every session."""

    rules = os.path.join(ROOT, ".claude", "rules")

    @staticmethod
    def run_hook():
        result = subprocess.run(
            ["bash", os.path.join(HOOKS, "codex-instructions.sh")],
            input="{}", capture_output=True, text=True, cwd=ROOT, check=False,
            env=dict(os.environ, CLAUDE_PROJECT_DIR=ROOT),
        )
        return result.returncode, result.stdout, result.stderr

    def test_drops_only_the_agents_import(self):
        text = codex_instructions.codex_text("@AGENTS.md\n\n# CLAUDE.md\n\nSee @AGENTS.md here.\n")
        self.assertNotIn("\n@AGENTS.md\n", "\n" + text)
        self.assertIn("See @AGENTS.md here.", text)
        self.assertTrue(text.startswith(codex_instructions.HEADER))

    def test_the_always_loaded_rules_are_the_five_without_paths(self):
        # CLAUDE.md's rule table lists these five as Always. A new rule without `paths:` lands
        # in every Codex session too, so adding one should be a decision, made here.
        self.assertEqual(
            codex_instructions.always_loaded_rules(self.rules),
            ["code-quality.md", "content-and-ip.md", "engineering-prose.md",
             "response-style.md", "testing.md"],
        )

    def test_prints_claude_md_then_each_always_loaded_rule_and_no_scoped_rule(self):
        code, out, err = self.run_hook()
        self.assertEqual(code, 0, err)
        self.assertTrue(out.startswith(codex_instructions.HEADER))
        self.assertIn("## Standing prohibitions and the permission boundary", out)
        for name in sorted(os.listdir(self.rules)):
            with open(os.path.join(self.rules, name), encoding="utf-8") as f:
                text = f.read()
            if text.startswith("---\n"):
                self.assertNotIn(f".claude/rules/{name} (always loaded)", out)
                self.assertNotIn(text.split("\n---\n", 1)[1].strip(), out)
            else:
                self.assertIn(f".claude/rules/{name} (always loaded)", out)
                self.assertIn(text.strip(), out)

    def test_output_fits_the_configured_spill_threshold(self):
        with open(os.path.join(ROOT, ".codex", "config.toml"), encoding="utf-8") as f:
            config = f.read()
        block = config[config.index("codex-instructions.sh"):]
        limit = int(re.search(r"^additionalContextLimit = (\d+)$", block, re.M).group(1))
        _, out, _ = self.run_hook()
        # Codex estimates tokens as ceil(bytes / 4) (codex-rs utils/string/src/truncate.rs).
        self.assertLessEqual((len(out.encode()) + 3) // 4, limit)


class CodexMirrors(unittest.TestCase):
    """Codex agents and skills point at .claude/ and repeat only the trigger description."""

    def test_every_claude_agent_has_a_codex_agent_with_the_same_description(self):
        agents = os.path.join(ROOT, ".claude", "agents")
        for name in sorted(os.listdir(agents)):
            meta = front_matter(os.path.join(agents, name))
            with open(os.path.join(ROOT, ".codex", "agents", meta["name"] + ".toml")) as f:
                toml = f.read()
            description = json.loads(re.search(r'^description = (".*")$', toml, re.M).group(1))
            self.assertEqual(description, meta["description"], name)
            self.assertIn(f"`.claude/agents/{name}`", toml)

    def test_every_claude_command_has_an_explicit_only_codex_skill(self):
        commands = os.path.join(ROOT, ".claude", "commands")
        for name in sorted(os.listdir(commands)):
            skill_dir = os.path.join(ROOT, ".agents", "skills", name[:-3])
            meta = front_matter(os.path.join(skill_dir, "SKILL.md"))
            command = front_matter(os.path.join(commands, name))
            self.assertTrue(
                json.loads(meta["description"]).startswith(command["description"] + "."), name,
            )
            with open(os.path.join(skill_dir, "SKILL.md")) as f:
                self.assertIn(f"`.claude/commands/{name}`", f.read())
            with open(os.path.join(skill_dir, "agents", "openai.yaml")) as f:
                self.assertIn("allow_implicit_invocation: false", f.read())


    def test_every_codex_agent_and_skill_has_a_claude_source(self):
        for name in os.listdir(os.path.join(ROOT, ".codex", "agents")):
            self.assertTrue(name.endswith(".toml"), name)
            source = os.path.join(ROOT, ".claude", "agents", name[:-5] + ".md")
            self.assertTrue(os.path.isfile(source), f".codex/agents/{name} has no {source}")
        for name in os.listdir(os.path.join(ROOT, ".agents", "skills")):
            source = os.path.join(ROOT, ".claude", "commands", name + ".md")
            self.assertTrue(os.path.isfile(source), f".agents/skills/{name} has no {source}")

    def test_only_agents_that_neither_edit_nor_build_ask_for_a_read_only_sandbox(self):
        # cargo and just write target/ and ~/.cargo, so an agent whose instructions run them
        # cannot work in a read-only sandbox.
        agents = os.path.join(ROOT, ".claude", "agents")
        for name in sorted(os.listdir(agents)):
            path = os.path.join(agents, name)
            meta = front_matter(path)
            tools = {t.strip() for t in meta["tools"].split(",")}
            with open(path, encoding="utf-8") as f:
                builds = re.search(r"`(cargo|just|pnpm) [a-z]", f.read()) is not None
            with open(os.path.join(ROOT, ".codex", "agents", meta["name"] + ".toml")) as f:
                read_only = re.search(r'^sandbox_mode = "read-only"$', f.read(), re.M) is not None
            self.assertEqual(read_only, not ({"Edit", "Write"} & tools) and not builds, name)


class CodexExecPolicy(unittest.TestCase):
    def test_only_read_only_git_status_is_allowed(self):
        # A Codex allow rule also runs the command outside the sandbox, and a prefix rule cannot
        # exclude an argument, so only commands with no writing argument belong here.
        with open(os.path.join(ROOT, ".codex", "rules", "stepclimb.rules")) as f:
            rules = f.read()
        patterns = re.findall(r"^\s*pattern = (\[.*\]),$", rules, re.M)
        self.assertEqual(patterns, ['["git", "status"]'])
        self.assertNotIn('"forbidden"', rules)


class RulesFitCodexPathRules(unittest.TestCase):
    rules = os.path.join(ROOT, ".claude", "rules")

    def test_no_rule_is_a_symlink(self):
        for name in os.listdir(self.rules):
            self.assertFalse(os.path.islink(os.path.join(self.rules, name)), name)

    def test_front_matter_is_closed_and_bodies_fit_the_budget(self):
        for name in sorted(os.listdir(self.rules)):
            with open(os.path.join(self.rules, name), encoding="utf-8") as f:
                text = f.read()
            body = text
            if text.startswith("---\n"):
                end = text.find("\n---\n", 4)
                self.assertNotEqual(end, -1, f"{name}: front matter not closed")
                body = text[end + 5:]
            self.assertLessEqual(len(body.strip()), PATH_RULES_MAX_RULE_CHARS, name)


class Goals(unittest.TestCase):
    """The agent goal prompts in .claude/goals/ (CLAUDE.md -> Agents) fit a goal field."""

    MAX_CHARS = 4000

    def test_the_three_goals_exist_and_fit(self):
        goals = os.path.join(ROOT, ".claude", "goals")
        self.assertEqual(sorted(os.listdir(goals)),
                         ["regression-checker.md", "ticket-reviewer.md", "ticket-worker.md"])
        for name in os.listdir(goals):
            with open(os.path.join(goals, name), encoding="utf-8") as f:
                text = f.read()
            self.assertLessEqual(len(text), self.MAX_CHARS, name)
            self.assertNotRegex(text, r"(?i)co-authored-by", name)


if __name__ == "__main__":
    unittest.main()
