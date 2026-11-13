# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the guard hooks both Claude Code and Codex run. Run with `just agents-check`.

Both tools refuse a tool call only when a hook exits 2, so each guard must exit 2 on a hit and on
any input it cannot check.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib

LIB = os.path.dirname(os.path.abspath(__file__))
HOOKS = os.path.dirname(LIB)
ROOT = os.path.dirname(os.path.dirname(HOOKS))
# Assembled so this file does not itself carry the trailer the hooks refuse.
CODEX_TRAILER = "Co-authored" + "-by: Codex"


def link_dir(target, link):
    """A directory link at `link` to `target`: a symlink, or on Windows a junction.

    Windows lets a user create a symlink only with Developer Mode or an admin's privilege; a
    junction needs neither, and `os.path.realpath` resolves it as it does a symlink.
    """
    if os.name == "nt":
        import _winapi

        _winapi.CreateJunction(target, link)
    else:
        os.symlink(target, link)


def git_bash_form(path):
    """`path` as typed in Git Bash on Windows (`/c/...`); elsewhere unchanged."""
    if os.name == "nt" and path[1:2] == ":":
        return "/" + path[0].lower() + path[2:].replace("\\", "/")
    return path


def run(script, payload, env=None, cwd=ROOT):
    raw = payload if isinstance(payload, str) else json.dumps(payload)
    result = subprocess.run(
        ["bash", os.path.join(HOOKS, script)],
        input=raw, capture_output=True, text=True, cwd=cwd, check=False,
        env=None if env is None else {**os.environ, **env},
    )
    return result.returncode, result.stderr


class ContentGuard(unittest.TestCase):
    def setUp(self):
        # A throwaway repo: content-guard finds the root by its .git entry.
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = self.tmp.name
        for d in (".git", "references", "docs"):
            os.mkdir(os.path.join(self.repo, d))
        link_dir(os.path.join(self.repo, "references"), os.path.join(self.repo, "docs", "refs"))

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, path):
        return run("content-guard.sh", {
            "cwd": self.repo, "tool_name": "Write", "tool_input": {"file_path": path},
        })

    def test_blocks_references_in_any_case(self):
        # macOS and Windows file systems ignore case.
        for path in ("references/x.pdf", "References/x.pdf", "REFERENCES/x.pdf"):
            code, err = self.write(path)
            self.assertEqual(code, 2, path)
            self.assertIn("local-only reference material", err)

    def test_blocks_a_path_through_a_symlink_into_references(self):
        code, err = self.write("docs/refs/x.pdf")
        self.assertEqual(code, 2, err)

    def test_allows_the_index_in_any_case_and_other_paths(self):
        for path in ("references/INDEX.md", "References/index.md", "docs/a.md"):
            code, err = self.write(path)
            self.assertEqual(code, 0, f"{path}: {err}")

    def test_blocks_private_data_in_any_case(self):
        for path in ("private/members.csv", "Private/dump.sql", "PRIVATE/x/y.json"):
            code, err = self.write(path)
            self.assertEqual(code, 2, path)
            self.assertIn("local-only private data", err)

    def test_allows_the_private_readme(self):
        code, err = self.write("private/README.md")
        self.assertEqual(code, 0, err)

    def test_blocks_through_a_references_symlink_that_leaves_the_repo(self):
        with tempfile.TemporaryDirectory() as outside:
            os.rmdir(os.path.join(self.repo, "references"))
            link_dir(outside, os.path.join(self.repo, "references"))
            code, err = self.write("references/x.pdf")
        self.assertEqual(code, 2, err)
        self.assertIn("local-only reference material", err)

    def test_blocks_through_a_references_symlink_to_another_repo(self):
        with tempfile.TemporaryDirectory() as other:
            os.mkdir(os.path.join(other, ".git"))
            os.rmdir(os.path.join(self.repo, "references"))
            link_dir(other, os.path.join(self.repo, "references"))
            code, err = self.write("references/x.pdf")
        self.assertEqual(code, 2, err)

    def test_blocks_under_a_nested_repo_inside_references(self):
        os.makedirs(os.path.join(self.repo, "references", "sub", ".git"))
        code, err = self.write("references/sub/x.pdf")
        self.assertEqual(code, 2, err)

    def test_blocks_in_a_worktree_inside_another_checkout(self):
        # An agent worktree lives under the main checkout and has a .git file, not a directory.
        tree = os.path.join(self.repo, ".claude", "worktrees", "agent")
        os.makedirs(os.path.join(tree, "references"))
        with open(os.path.join(tree, ".git"), "w") as f:
            f.write("gitdir: ../../../.git/worktrees/agent\n")
        code, err = run("content-guard.sh", {
            "cwd": tree, "tool_name": "Write", "tool_input": {"file_path": "references/x.pdf"},
        })
        self.assertEqual(code, 2, err)
        code, err = run("content-guard.sh", {
            "cwd": tree, "tool_name": "Write", "tool_input": {"file_path": "references/INDEX.md"},
        })
        self.assertEqual(code, 0, err)

    def test_blocks_against_the_project_dir_without_a_git_entry(self):
        os.rmdir(os.path.join(self.repo, ".git"))
        result = subprocess.run(
            ["bash", os.path.join(HOOKS, "content-guard.sh")],
            input=json.dumps({"cwd": self.repo, "tool_input": {"file_path": "references/x.pdf"}}),
            capture_output=True, text=True, cwd=ROOT, check=False,
            env=dict(os.environ, CLAUDE_PROJECT_DIR=self.repo),
        )
        self.assertEqual(result.returncode, 2, result.stderr)

    def test_blocks_a_non_object_tool_input(self):
        code, err = run("content-guard.sh", {"cwd": self.repo, "tool_input": "x"})
        self.assertEqual(code, 2)
        self.assertIn("blocked", err)

    def test_blocks_when_the_hook_crashes(self):
        # A list cwd makes the path join raise TypeError; run_guard must turn that into exit 2.
        code, err = run("content-guard.sh", {"cwd": [self.repo], "tool_input": {"file_path": "a"}})
        self.assertEqual(code, 2, err)
        self.assertIn("the hook failed (TypeError", err)


class EditHookMatchers(unittest.TestCase):
    """Every Claude Code tool that writes a file reaches the content guard and the formatter."""

    def matcher(self, event, script):
        with open(os.path.join(ROOT, ".claude", "settings.json"), encoding="utf-8") as f:
            entries = json.load(f)["hooks"][event]
        found = [e["matcher"] for e in entries
                 if any(script in h.get("command", "") for h in e["hooks"])]
        self.assertEqual(len(found), 1, f"{script} should appear in one {event} entry")
        return found[0]

    def assert_covers(self, matcher, tools):
        # Claude Code reads a matcher with `|` as a regex over the tool name.
        for tool in tools:
            self.assertRegex(tool, f"^(?:{matcher})$", f"{matcher!r} misses {tool}")

    def test_content_guard_sees_every_edit_tool(self):
        matcher = self.matcher("PreToolUse", "content-guard.sh")
        self.assert_covers(matcher, ("Write", "Edit", "MultiEdit", "NotebookEdit"))

    def test_the_formatter_sees_every_text_edit_tool(self):
        matcher = self.matcher("PostToolUse", "format-on-edit.sh")
        self.assert_covers(matcher, ("Write", "Edit", "MultiEdit"))


class RunGuard(unittest.TestCase):
    def test_an_unexpected_error_blocks(self):
        sys.path.insert(0, LIB)
        import hookio

        def crash():
            raise RuntimeError("boom")

        with self.assertRaises(SystemExit) as exit_:
            hookio.run_guard("test", crash)
        self.assertEqual(exit_.exception.code, 2)


class GitGuard(unittest.TestCase):
    def test_blocks_input_it_cannot_check(self):
        for payload in ("not json", {"tool_input": "x"}, {"tool_input": {"command": ["git"]}}):
            code, err = run("git-guard.sh", payload)
            self.assertEqual(code, 2, payload)
            self.assertIn("blocked", err)

    def test_blocks_when_the_hook_crashes(self):
        # A list cwd makes current_branch raise TypeError; run_guard must turn that into exit 2.
        code, err = run("git-guard.sh", {"cwd": [ROOT], "tool_input": {"command": "git push"}})
        self.assertEqual(code, 2, err)
        self.assertIn("the hook failed (TypeError", err)

    def repo_with_local_only_files(self):
        """A throwaway repo on branch `work` that ignores references/ and private/, with a file
        in each, and docs/refs, a link to references/ (a junction on Windows)."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        repo = tmp.name
        for path in ("references/x.pdf", "private/a.csv", "docs/a.md", "crates/references.rs"):
            os.makedirs(os.path.dirname(os.path.join(repo, path)), exist_ok=True)
            open(os.path.join(repo, path), "w").close()
        with open(os.path.join(repo, ".gitignore"), "w") as f:
            f.write("references/\nprivate/\n")
        subprocess.run(["git", "init", "-q", "-b", "work", repo], check=True)
        link_dir(os.path.join(repo, "references"), os.path.join(repo, "docs", "refs"))
        return repo

    def add(self, command, cwd, env=None):
        # The hook runs in the command's directory, as Claude Code and Codex start it in the
        # project, so a path the guard resolves against the process directory lands in a repo.
        return run("git-guard.sh", {"cwd": cwd, "tool_input": {"command": command}}, env, cwd)

    def assert_refused(self, commands, cwd, message, env=None):
        for command in commands:
            code, err = self.add(command, cwd, env)
            self.assertEqual(code, 2, command)
            self.assertIn(message, err, command)

    def test_blocks_a_forced_add_of_references_or_private_data(self):
        # Paths read against the directory the add runs in, up to the repository top: a
        # backslash separator as on Windows, any case, `:/`, `..`, -C, an absolute path, and the
        # `stage`, `git.exe` and `--fo` spellings.
        repo = self.repo_with_local_only_files()
        docs = os.path.join(repo, "docs")
        absolute = os.path.join(repo, "references", "x.pdf")
        self.assert_refused((
            "git add -f references/x.pdf",
            "git add --force ./private/a.csv",
            "git add -f Private/a.csv",
            "git add -f :/",
            "git add -f .",
            "git -C docs add -f ..",
            "git -C references -C . add -f x.pdf",
            "git --config-env foo.bar=HOME add -f " + "references/x.pdf",
            "git --attr-source HEAD add -f " + "references/x.pdf",
            "git stage -f references/x.pdf",
            "git.exe add -f references/x.pdf",
            "git add --fo references",
            "git add -f '>' references/x.pdf",
            "git add -f '>/../references/x.pdf'",
            "git add -f '2>/../references/x.pdf'",
            "git add -f references/index.md",
            "git add -f references/x.pdf 2>&1",
            # Quoting bash removes before git runs.
            "git add -f $'" + "references/x.pdf'",
            "git add -f $'\\x72eferences/x.pdf'",
            "git add -f refer$''ences/x.pdf",
            "g\\it add -f " + "references/x.pdf",
            "$'git' add -f " + "references/x.pdf",
            # Reserved words and `command` leave git in command position.
            "if true; then git add -f " + "references/x.pdf; fi",
            "! git add -f " + "references/x.pdf",
            "time -p git add -f " + "references/x.pdf",
            "command git add -f " + "references/x.pdf",
            "{ git add -f " + "references/x.pdf; }",
            # A git executable by another name.
            "/usr/bin/git add -f " + "references/x.pdf",
            "GIT.EXE ADD -f " + "references/x.pdf",
            "git-add -f " + "references/x.pdf",
            "/usr/lib/git-core/git-stage -f " + "references/x.pdf",
            # A cd, followed in order; a single & backgrounds only the command before it.
            "cd references && git add -f x.pdf",
            "cd docs && git add -f ../" + "references/x.pdf",
            "cd docs && cd .. && git add -f " + "references/x.pdf",
            "command cd references && git add -f x.pdf",
            "'cd' references && git add -f x.pdf",
            "\\cd references && git add -f x.pdf",
            "cd references || true; git add -f x.pdf",
            "cd docs || git add -f " + "references/x.pdf",
            "cd docs | true && git add -f " + "references/x.pdf",
            "true | cd docs && git add -f " + "private/a.csv",
            "true & git add -f " + "references/x.pdf",
            "cd references & git add -f " + "references/x.pdf",
            "cd references\ngit add -f x.pdf",
            "pushd -n references && git add -f " + "references/x.pdf",
            # A git alias, from -c.
            "git -c alias.put='add -f' put " + "references/x.pdf",
            "git -c alias.put='add -f' -c alias.p=put p " + "references/x.pdf",
            # `!` inverts a cd's status; lastpipe keeps a pipeline's last cd.
            "! cd references || git add -f x.pdf",
            "shopt -s lastpipe; true | cd references; git add -f x.pdf",
            # A case pattern with its optional `(`.
            "case x in (*) cd references;; esac; git add -f x.pdf",
            # A program that runs git in the same directory.
            "nice -n 10 git add -f " + "references/x.pdf",
            "timeout 300 git add -f " + "references/x.pdf",
            "env git add -f " + "references/x.pdf",
        ), repo, "local-only references or private data")
        self.assert_refused((
            "git add -f :/references/x.pdf",
            "git add -f ../references/x.pdf",
            f"git add -f '{absolute}'",
            f"git add -f '{git_bash_form(absolute)}'",
        ), docs, "local-only references or private data")
        backslash = "git add -f 'references\\x.pdf'"
        if os.name == "nt":  # a separator there
            self.assert_refused((backslash,), repo, "local-only references or private data")
        else:  # a glob escape there: `reference\s` is references
            self.assert_refused((backslash, "git add -f 'reference\\s/x.pdf'"), repo,
                                "cannot be checked")
        if os.name != "nt":  # off Windows, git reads `C:` as a folder name and `C:/..` folds away
            self.assert_refused(("git add -f 'C:/../references/x.pdf'",), repo,
                                "local-only references or private data")

    def test_follows_tilde_and_a_bare_cd_to_home(self):
        repo = self.repo_with_local_only_files()
        self.assert_refused(("git add -f ~/references/x.pdf", "cd ~ && git add -f references",
                             "cd && git add -f references/x.pdf",
                             "cd ~/docs && git add -f ../private/a.csv"),
                            os.path.join(repo, "docs"), "local-only references or private data",
                            env={"HOME": git_bash_form(repo)})

    def test_a_home_the_text_sets_is_not_followed(self):
        repo = self.repo_with_local_only_files()
        self.assert_refused((f"HOME='{git_bash_form(repo)}'; git add -f ~/references/x.pdf",
                             f"HOME='{git_bash_form(repo)}'; cd && git add -f references/x.pdf",
                             "pushd references && pushd .. && pushd && git add -f x.pdf"),
                            repo, "cannot be checked",
                            env={"HOME": git_bash_form(os.path.join(repo, "docs"))})

    def test_never_stats_a_network_path(self):
        # A UNC path's stat can wait on the network past Codex's 30 s hook timeout.
        sys.path.insert(0, LIB)
        import git_add_paths
        import git_guard
        import hookio

        repo = self.repo_with_local_only_files()
        real = hookio.repo_root

        def local_only(path):
            self.assertFalse(path.replace("\\", "/").startswith("//"), path)
            return real(path)

        hookio.repo_root = local_only
        try:
            for command in ("(git add -f //host/share/" + "references/x.pdf)",
                            "cd //host/share && git add -f x.pdf",
                            "git -C //host/share add -f x.pdf"):
                with self.assertRaises(git_add_paths.Refusal, msg=command):
                    git_guard.Checker(repo).run(command)
        finally:
            hookio.repo_root = real

    def test_expands_git_aliases_from_config(self):
        repo = self.repo_with_local_only_files()
        for key, value in (("alias.put", "add -f"), ("alias.p", "put"),
                           ("alias.sh", "!git add -f")):
            subprocess.run(["git", "-C", repo, "config", key, value], check=True)
        self.assert_refused(("git put references/x.pdf", "git p references/x.pdf",
                             "git PUT references/x.pdf", "git -C docs put ../references/x.pdf"),
                            repo, "local-only references or private data")
        self.assert_refused(("git sh references/x.pdf",), repo, "cannot be checked")
        self.assertEqual(self.add("git put docs/a.md", repo)[0], 0)

    def test_blocks_a_forced_add_whose_paths_the_text_cannot_settle(self):
        repo = self.repo_with_local_only_files()
        self.assert_refused((
            "git add -f ':!docs'", "git add -f '*.pdf'", "git add -Af",
            "git add -Af 2>&1", "git add -Af > /dev/null", "git add -f --al",
            "git add -f --no-ignore-removal", "git add -f --pathspec-from=-",
            "git add -f --pathspec-from-file=list",
            "git add -f ':/:'", "git add -f ':/!docs'", "git add -f ':/^docs'",
            "git add -f ':/:" + "references/x.pdf'",
            "git add -f {" + "references,docs}/x.pdf",
            "git add -f " + "references{,}/x.pdf",
            "git add -f $D/x.pdf",
            "git add -f `echo " + "references/x.pdf`",
            "git add -f -$X docs/a.md",
            "git add -f ~nosuchuser/x.pdf",
            # A path outside the repository, which git would read through a link.
            "git add -f /nonexistent/" + "references/x.pdf",
            "git -C .. add -f x/references/x.pdf",
            # A subshell, substitution, command string or eval.
            "(cd references && git add -f x.pdf)",
            "(git add -f docs/a.md)",
            "echo $(git add -f docs/a.md)",
            "echo `git add -f docs/a.md`",
            "echo \"$(git add -f docs/a.md)\"",
            "cat <(git add -f docs/a.md)",
            "bash -c 'git add -f " + "references/x.pdf'",
            "sh -ec \"cd references && git add -f x.pdf\"",
            "eval git add -f " + "references/x.pdf",
            "eval 'git add -f " + "references/x.pdf'",
            "cat <<EOF\n$(git add -f docs/a.md)\nEOF",
            "echo ${x:-$(git add -f docs/a.md)}",
            "echo \"${x:-$(git add -f docs/a.md)}\"",
            "echo $(( $(git add -f docs/a.md) + 0 ))",
            "eval \"$(cat <<'EOF'\ngit add -f " + "references/x.pdf\nEOF\n)\"",
            "bash -c \"$(printf 'git add -f " + "references/x.pdf')\"",
            "CMD='git add -f " + "references/x.pdf'; bash -c \"$CMD\"",
            "CMD='git add -f " + "references/x.pdf'; eval \"$CMD\"",
            "printf 'git add -f " + "references/x.pdf' | sh",
            # A brace or variable bash expands into the command word, subcommand or options.
            "git add {-f," + "references/x.pdf}",
            "git {add,-f} " + "references/x.pdf",
            "{git,add,-f," + "references/x.pdf}",
            "git $S -f " + "references/x.pdf",
            "F=-f; git add $F " + "references/x.pdf",
            "git add \"$F\"",
            # Through another program, or a command word the text does not settle.
            "bash <<'EOF'\ngit add -f " + "references/x.pdf\nEOF",
            "sh <<< 'git add -f " + "references/x.pdf'",
            "env -C references git add -f x.pdf",
            "sudo git add -f docs/a.md",
            "echo docs/a.md | xargs git add -f",
            "find . -name x.pdf -exec git add -f {} ';'",
            "env sh -c 'git add -f x.pdf'",
            "$G add -f " + "references/x.pdf",
            "\"$(command -v git)\" add -f " + "references/x.pdf",
            "/usr/bin/gi? add -f " + "references/x.pdf",
            # A cd the hook cannot follow.
            "cd $D && git add -f x.pdf",
            "cd - && git add -f x.pdf",
            "pushd references && popd && git add -f x.pdf",
            "for d in a; do cd references; done; git add -f x.pdf",
            "while false; do git add -f x.pdf; cd references; done",
            "f() { cd references; }; f; git add -f x.pdf",
            "f() { git add -f " + "references/x.pdf; }; f",
            "function f { git add -f x.pdf; }; f",
            "bash -o pipefail -c 'git add -f " + "references/x.pdf'",
            "bash -O extglob -lc 'git add -f " + "references/x.pdf'",
            # A link inside the repository, or a Windows alias of a name.
            "git -C docs/refs add -f x.pdf",
            "cd docs/refs && git add -f x.pdf",
            "git add -f docs/refs/x.pdf",
            "git add -f docs/refs/",
            "git -C docs/refs/.. add -f " + "x.pdf",
            "git add -f REFERE~1/x.pdf",
            "git -C REFERE~1 add -f x.pdf",
            "git add -f 'references./x.pdf'",
            "git add -f 'references /x.pdf'",
            "git add -f C:" + "references/x.pdf",
            # An alias whose expansion the text cannot read.
            "git -c alias.x='!git add -f " + "references/x.pdf' x",
            "git --config-env alias.put=V put " + "references/x.pdf",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.put git put x.pdf",
            "git -c include.path=/x/evil.cfg put " + "references/x.pdf",
            "git --config-env include.path=EVIL put " + "references/x.pdf",
            "HOME=/x git put " + "references/x.pdf",
            "XDG_CONFIG_HOME=/x git put " + "references/x.pdf",
            "git " + " ".join(f"-c alias.a{n}=a{n + 1}" for n in range(70)) + " a0 x.pdf",
            # CDPATH set in the text; HOME set in the text.
            "CDPATH=references cd sub && git add -f ../x.pdf",
            "export CDPATH=docs; cd refs && git add -f x.pdf",
            "HOME=/x; cd ~ && git add -f x.pdf",
            # A network path, whose stat could wait on the network.
            "git -C //host/share add -f x.pdf",
            # The work tree or git directory moved, forced or not.
            "git --work-tree=references add -f x.pdf",
            "git --work-tree references add x.pdf",
            "GIT_WORK_TREE=references git add x.pdf",
            "export GIT_WORK_TREE=references; git add x.pdf",
            "git -c core.worktree=references add x.pdf",
            "git --git-dir=.git add -f x.pdf",
            # bash 5.3 runs `${ cmd; }` in this shell; a case pattern's `)` closes nothing.
            "echo ${ git add -f docs/a.md; }",
            "echo ${| git add -f docs/a.md; }",
            "${ cd references; }; git add -f x.pdf",
            "echo $(case a in a) git add -f docs/a.md;; esac)",
            "(cd references; case a in a) git add -f x.pdf;; esac)",
            # A shell reading its script from an option's value, stdin or a substitution.
            "bash --init-file /dev/null -c 'git add -f " + "references/x.pdf'",
            "bash --rcfile /dev/null -c 'git add -f " + "references/x.pdf'",
            "bash /dev/stdin <<'EOF'\ngit add -f " + "references/x.pdf\nEOF",
            ". /dev/stdin <<< 'git add -f " + "references/x.pdf'",
            "source <(echo 'git add -f " + "references/x.pdf')",
            "bash <(echo 'git add -f " + "references/x.pdf')",
            "X='git add -f " + "references/x.pdf'; bash <<< \"$X\"",
            "X='git add -f " + "references/x.pdf'; bash <<EOF\n$X\nEOF",
            # A forced add written in a word another program may run.
            "X='git add -f " + "references/x.pdf'; $X",
            "trap 'git add -f " + "references/x.pdf' EXIT",
            "script -qc 'git add -f " + "references/x.pdf' /dev/null",
            "flock /tmp/l -c 'git add -f " + "references/x.pdf'",
            "git -c core.fsmonitor='git add -f " + "references/x.pdf; false' status",
            "env -S 'git add -f " + "references/x.pdf'",
            "G=git; F=-f; $G add $F " + "references/x.pdf",
            # env told to change directory by an abbreviated option.
            "env --ch=references git add -f x.pdf",
            # Variables set without NAME=.
            ": ${CDPATH:=references}; cd sub && git add -f x.pdf",
            "read CDPATH <<< references; cd sub && git add -f x.pdf",
            ": ${GIT_WORK_TREE:=.}; export GIT_WORK_TREE; cd references && git add x.pdf",
            "HOME=/x; cd && git add -f x.pdf",
            # A shell alias runs wherever its name is used.
            "alias c='cd references'\nc && git add -f x.pdf",
            # A link or git config the command makes before git runs, which the hook reads now.
            "ln -s references r && cd r && git add -f x.pdf",
            "ln -s references r && git -C r add -f x.pdf",
            "git config alias.put 'add -f' && git put " + "references/x.pdf",
            "git config core.worktree \"$PWD/references\" && git add x.pdf",
            "git config set core.worktree \"$PWD/references\" && git add x.pdf",
            "printf '[alias]\\n put = add -f\\n' >> .git/config; git put x.pdf",
            # An alias value git cannot split; a non-ASCII alias of references.
            "git -c alias.q=\"log --format='%h\" q",
            "git add -f 'reference\u017f/x.pdf'",
            # `$((` closed by `) )` is a substitution holding a subshell, not arithmetic.
            "echo $((git add -f docs/a.md) )",
            "echo $((cd references && git add -f x.pdf) )",
            # A function's body, whatever the function is named.
            "git() { command git add -f docs/a.md; }; git status",
            "function git { command git add -f docs/a.md; }; git status",
            "echo() { git add -f docs/a.md; }; echo hi",
            "grep() { git add -f docs/a.md; }; grep x y",
            # A shell's script from a `<` of a substitution, an --rcfile one, or -s with arguments.
            "bash < <(echo 'git add -f " + "references/x.pdf')",
            "bash 0< <(echo 'git add -f " + "references/x.pdf')",
            "bash --rcfile <(echo 'git add -f " + "references/x.pdf') -i",
            "bash -s x <<'X'\ngit add -f " + "references/x.pdf\nX",
            # Text run in this shell, where its cd sticks.
            "eval cd references; git add -f x.pdf",
            ". /dev/stdin <<< 'cd references'; git add -f x.pdf",
            "trap 'cd references' DEBUG; git add -f x.pdf",
            "echo 'cd references' > c.sh; . c.sh; git add -f x.pdf",
            # A command word or arithmetic value loaded from text the hook reads only whole.
            "read -ra a <<< 'git add -f " + "references/x.pdf'; \"${a[@]}\"",
            "mapfile -t a <<< 'git'; a+=(add -f " + "references/x.pdf); \"${a[@]}\"",
            "read x <<< 'a[$(git add -f " + "references/x.pdf)]'; echo $((x))",
            # cdable_vars, and a bare pushd, which swaps the stack.
            "shopt -s cdable_vars; r=references; cd r; git add -f x.pdf",
            # A startup file a shell reads; a here-document piped into one.
            "A='git add'; B='-f " + "references/x.pdf'; BASH_ENV=f bash -c true",
            "cat <<EOF | bash\ngit add -f " + "references/x.pdf\nEOF",
            "tee -a .git/config <<< '[alias] put = add -f'; git put " + "references/x.pdf",
        ), repo, "cannot be checked")
        self.assert_refused(("cd references && git add -f x.pdf",), repo, "cannot be checked",
                            env={"CDPATH": repo})
        self.assert_refused(("(git add -f docs/a.md)", "cd $D && git add -f x.pdf"), repo,
                            "after a `cd` the hook cannot follow, or in a subshell")
        self.assert_refused(("alias g=git; g add -f " + "references/x.pdf",
                             "alias p='git put'"), repo, "the shell alias")
        self.assert_refused(("alias ga='git add -f " + "references/x.pdf'",), repo,
                            "cannot be checked")
        self.assert_refused(("alias gp='git push origin next'",), repo, "push to next")
        # Commands a shell or eval runs are checked, the push guard too.
        self.assert_refused(("bash -c 'git push origin next'",
                             "bash -o pipefail -c 'git push origin next'",
                             "bash --rcfile /dev/null -c 'git push origin next'",
                             "eval git push origin next", "eval 'git push origin next'",
                             "echo $(( $(git push origin next) + 0 ))",
                             "echo $((git push origin next) )"),
                            repo, "push to next")
        # A forced add split across variables, so no single word holds it.
        self.assert_refused(("A='git add'; B='-f " + "references/x.pdf'; bash -c \"$A $B\"",
                             "A='git add'; B='-f " + "references/x.pdf'; eval \"$A $B\"",
                             "A='git add'; B='-f " + "references/x.pdf'; bash <<< \"$A $B\"",
                             "A='git add'; B='-f " + "references/x.pdf'; bash <<EOF\n$A $B\nEOF"),
                            repo, "cannot be checked")
        self.assert_refused(("git add -f 'references/x.pdf",), repo, "could not read")

    def test_blocks_an_add_autocorrect_may_run(self):
        repo = self.repo_with_local_only_files()
        subprocess.run(["git", "-C", repo, "config", "help.autocorrect", "immediate"], check=True)
        # git runs `dd` as `add` (the only command one edit away); any subcommand that is not
        # a command or alias may run as another.
        self.assert_refused(("git dd -f " + "references/x.pdf", "git dd docs/a.md"), repo,
                            "help.autocorrect")
        self.assertEqual(self.add("git status", repo)[0], 0)

    @unittest.skipIf(os.name == "nt", "a git-<name> program on PATH needs an .exe there")
    def test_autocorrect_passes_a_git_program_on_path(self):
        repo = self.repo_with_local_only_files()
        bindir = os.path.join(repo, "bin")
        os.mkdir(bindir)
        with open(os.path.join(bindir, "git-hello"), "w") as f:
            f.write("#!/bin/sh\n")
        os.chmod(os.path.join(bindir, "git-hello"), 0o755)
        code, err = self.add("git -c help.autocorrect=1 hello", repo,
                             env={"PATH": bindir + os.pathsep + os.environ["PATH"]})
        self.assertEqual(code, 0, err)

    def test_blocks_an_add_autocorrect_from_the_command_may_run(self):
        repo = self.repo_with_local_only_files()
        self.assert_refused(("git -c help.autocorrect=immediate dd -f " + "references/x.pdf",
                             "git -c help.autocorrect=immediate -c alias.putt='add -f' put "
                             + "references/x.pdf",
                             "git -c help.autocorrect=immediate psuh origin HEAD:next"),
                            repo, "help.autocorrect")
        self.assert_refused(("git config help.autocorrect immediate && git dd -f "
                             + "references/x.pdf",), repo, "cannot be checked")
        self.assert_refused(("git --config-env=help.autocorrect=SHLVL addd -f "
                             + "references/x.pdf",), repo, "help.autocorrect")

    # Ordinary commands the guard must pass: the list the reviewers were given.
    ORDINARY = (
        "git add -f docs/a.md", "git add -f crates/references.rs", "git add -f :/docs",
        "git add -f docs/a.md 2>&1", "git add -f docs/a.md > /dev/null 2>&1",
        "git add -f docs/a.md &> /dev/null", "git add -f docs/a.md 2> /tmp/x.log",
        "git add -f references/INDEX.md", "git add references/x.pdf",
        "git add -- -f private/a.csv", "git add -u", "git add -A docs", "git add -A",
        "git add --chmod=+x docs/a.md", "git add -f --chmod=+x docs/a.md", "git add .",
        "git add -N docs/a.md", "git add -f \"docs/a.md\"", "git add -f ./docs/a.md",
        "git -C docs add -f a.md",
        "git add -f docs/a.md && git status --short", "git add -f docs/a.md; git diff --cached",
        "cd docs && git add -f a.md", "cd docs; git add -f a.md",
        "cd references & git add -f docs/a.md", "git add -f docs/a.md & wait",
        "(cd docs && ls) && git add -f docs/a.md", "echo $(git rev-parse HEAD)",
        "git commit -F - <<'EOF'\ngit add -f references/x.pdf\n"
        "(cd references && git add -f x)\nEOF",
        "git commit -m \"$(cat <<'EOF'\nfix: refuse git add -f references/x.pdf\nEOF\n)\"",
        "git log --oneline | head -5", "git status && git diff --stat",
        "for f in a b; do echo $f; done && git add -f docs/a.md",
        "GIT_TRACE=1 git add -f docs/a.md", "ls ~ && git add -f docs/a.md",
        "git add -f docs/a.md # then references/x.pdf", "sh -c 'cargo build'",
        "bash -c \"git status\"", "env FOO=1 cargo test", "git stash push -u -m tag",
        "git worktree list", "git lfs ls-files", "just ci 2>&1 | tail -3",
        "git push origin HEAD", "git -c color.ui=never add -f docs/a.md",
        "rm -f references/x.pdf", "git rm -f --cached docs/a.md", "git checkout -f work",
        "nice -n 10 git add -f docs/a.md", "timeout 300 git add -f docs/a.md",
        "GIT_CONFIG_NOSYSTEM=1 git add docs/a.md", "alias gs='git status'; ls",
        "alias ll='ls --digital'", "echo git add -f docs/a.md", "git add -f docs/notes~1.md",
        "echo $'\\UFFFFFFFF'", "bash <<'EOF'\ncargo build\nEOF", "true | cd docs; ls",
        "! git diff --quiet || echo changed", "case x in (a) echo a;; esac",
        "bash -c \"$CMD\"", "git add -f docs/a.md 2>&1 | tail -1",
        "coproc cd references; git add -f docs/a.md",
        "git add -- \"$f\"", "git add -- $(git ls-files -m)", "alias gr='git rerere'",
        "git config --get user.name && git add -f docs/a.md", "git config user.name x; git status",
        "git config --get-regexp '^alias\\.' add && git add -f docs/a.md",
        "git config --get-all core.worktree '.*'; git add docs/a.md",
        "ln -s a b; ls", "command -v git", "git switch -c feat && git status",
        "echo 'git add -f references/x.pdf'",
        "gh pr create --title 'Refuse git add -f references' --body x",
        "git commit -m 'Refuse git add -f references/x.pdf'", "echo ${x:-y} ${#x}",
        "case $x in (a|b) cd docs;; esac", "pushd -n docs && git add -f docs/a.md",
        "pushd -n references && git add -f docs/a.md",
        "git config user.email t@t && git add docs/a.md", "git config user.email t@t && git add .",
        "git add -A && git commit -m 'fix .gitconfig parsing'",
        "cat .git/config; git add docs/a.md",
        "git add docs/{a,b}.md", "rg -n 'git add -f' .claude/hooks", "echo $'\\c' git",
        "git -c help.autocorrect=1 status", "echo $(( 1 + 2 )) && git add -f docs/a.md",
    )

    def test_passes_ordinary_commands(self):
        repo = self.repo_with_local_only_files()
        # Off Windows git stages a link named last as the link itself.
        for command in self.ORDINARY + (() if os.name == "nt" else ("git add -f docs/refs",)):
            code, err = self.add(command, repo)
            self.assertEqual(code, 0, f"{command}: {err}")

    def test_reads_windows_paths(self):
        # A backslash is a separator on every OS; a drive path is absolute only on Windows,
        # where git.exe reads it so.
        sys.path.insert(0, LIB)
        import git_guard
        import hookio

        real = hookio.repo_root
        hookio.repo_root = lambda _where: "C:\\Repo"
        try:
            self.assertEqual(git_guard.repo_path("..\\private\\a.csv", "C:\\Repo\\docs"),
                             "private/a.csv")
            if os.name == "nt":
                self.assertEqual(
                    git_guard.repo_path("C:\\Repo\\References\\x.pdf", "C:\\Repo\\docs"),
                    "references/x.pdf")
                outside = git_guard.repo_path("D:\\other\\x", "C:\\Repo")
                self.assertTrue(outside.startswith("../"), outside)
        finally:
            hookio.repo_root = real

    def test_blocks_a_push_to_next_and_passes_status(self):
        self.assertEqual(run("git-guard.sh", {"cwd": ROOT, "tool_input": {
            "command": "git push origin HEAD:next"}})[0], 2)
        self.assertEqual(run("git-guard.sh", {"cwd": ROOT, "tool_input": {
            "command": "git status"}})[0], 0)


class Attribution(unittest.TestCase):
    def commit(self, message):
        return {"tool_name": "Bash", "tool_input": {"command": f'git commit -m "{message}"'}}

    def test_gate_blocks_a_bare_codex_trailer(self):
        code, err = run("attribution-gate.sh", self.commit(f"feat: x\n\n{CODEX_TRAILER}"))
        self.assertEqual(code, 2)
        self.assertIn("Codex", err)

    def test_commit_msg_hook_rejects_a_bare_codex_trailer(self):
        with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as f:
            f.write(f"feat: x\n\n{CODEX_TRAILER}\n")
        try:
            result = subprocess.run(
                ["bash", os.path.join(ROOT, ".githooks", "commit-msg"), f.name],
                capture_output=True, text=True, check=False,
            )
        finally:
            os.unlink(f.name)
        self.assertEqual(result.returncode, 1)

    def test_gate_passes_a_clean_commit(self):
        self.assertEqual(run("attribution-gate.sh", self.commit("feat: x"))[0], 0)

    def hits(self, text):
        script = f'. "{os.path.join(LIB, "attribution.sh")}"; attribution_hits >/dev/null'
        return subprocess.run(["bash", "-c", script], input=text, text=True, check=False).returncode

    def test_prose_and_names_containing_ai_pass(self):
        co = "Co-authored" + "-by"
        for text in (
            "docs(claude): describe the hook\n\nGenerated by the codex-instructions hook.",
            "the output is generated by codex-path-rules",
            "Gen" + "erated by codex-path-rules",
            "Drafted by Vainnor",
            f"{co}: Kailash <k@example.com>",
            f"{co}: Ai Nguyen <a@example.com>",
            "The rain in Spain, OpenAPI and FAIR data",
        ):
            self.assertEqual(self.hits(text), 1, text)

    def test_tool_and_ai_trailers_and_footers_block(self):
        co = "Co-authored" + "-by"
        gen = "Gen" + "erated"
        for text in (
            f"{co}: Claude <noreply@anthropic.com>",
            f"{co}: Codex <noreply@openai.com>",
            f"{co}: GPT-4o",
            f"{co}: Some AI <x@example.com>",
            f"{co.upper()}: AI",
            f'gh pr create --body "## Summary\\n\\n{gen} with Claude Code"',
            f"- {gen} with Claude Code",
            f"> {gen} with Claude Code",
            f"_{gen} with Claude Code_",
            f"**{gen} with Claude Code**",
            f"{gen} with [Claude Code](https://example.com)",
            f'git commit -m "{gen} by Codex"',
            f"{gen} by AI",
            "Draf" + "ted by Claude",
        ):
            self.assertEqual(self.hits(text), 0, text)

    def failing_grep_env(self, tmp):
        # A grep that fails only for the -n searches attribution_hits runs, so the gate's
        # pre-filter still works and attribution_hits returns 2.
        real = subprocess.run(
            ["bash", "-c", "command -v grep"], capture_output=True, text=True, check=True,
        ).stdout.strip()
        fake = os.path.join(tmp, "grep")
        with open(fake, "w") as f:
            f.write(f'#!/bin/sh\ncase " $* " in *" -inE "*|*" -nE "*) exit 2;; esac\nexec {real} "$@"\n')
        os.chmod(fake, 0o755)
        return dict(os.environ, PATH=tmp + os.pathsep + os.environ["PATH"])

    def test_gate_blocks_when_the_check_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = subprocess.run(
                ["bash", os.path.join(HOOKS, "attribution-gate.sh")],
                input=json.dumps(self.commit("feat: x")), capture_output=True, text=True,
                cwd=ROOT, env=self.failing_grep_env(tmp), check=False,
            )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("could not be checked", result.stderr)

    def test_commit_msg_hook_rejects_when_the_check_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            message = os.path.join(tmp, "msg.txt")
            with open(message, "w") as f:
                f.write("feat: x\n")
            result = subprocess.run(
                ["bash", os.path.join(ROOT, ".githooks", "commit-msg"), message],
                capture_output=True, text=True, env=self.failing_grep_env(tmp), check=False,
            )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("could not be checked", result.stderr)

    def test_a_failed_sed_counts_as_a_hit(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = os.path.join(tmp, "sed")
            with open(fake, "w") as f:
                f.write("#!/bin/sh\nexit 2\n")
            os.chmod(fake, 0o755)
            script = f'. "{os.path.join(LIB, "attribution.sh")}"; attribution_hits'
            result = subprocess.run(
                ["bash", "-c", script], input="feat: x\n", capture_output=True, text=True,
                env=dict(os.environ, PATH=tmp + os.pathsep + os.environ["PATH"]), check=False,
            )
        self.assertEqual(result.returncode, 2, result.stdout)

    def test_a_failed_check_counts_as_a_hit(self):
        # attribution_hits returns 2 when grep itself fails (here, an invalid pattern).
        script = (
            f'. "{os.path.join(LIB, "attribution.sh")}"; attribution_pattern="("; '
            'echo text | attribution_hits; echo "rc=$?"'
        )
        out = subprocess.run(["bash", "-c", script], capture_output=True, text=True).stdout
        self.assertIn("rc=2", out)


if __name__ == "__main__":
    unittest.main()
