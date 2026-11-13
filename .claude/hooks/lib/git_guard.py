# SPDX-License-Identifier: AGPL-3.0-or-later
"""PreToolUse(Bash): refuse git commands that bypass the PR workflow or commit local-only content.

Blocks: `git push` that targets main or next (by refspec, by HEAD or a bare push while on one of
them), a force push without --force-with-lease, a forced `git add` or `git stage` of a path under
references/ or private/ or of the whole tree, an add with its work tree or git directory moved,
and `git commit` while on main or next (AGENTS.md, Git workflow; content-and-ip.md).

It reads the command text and runs nothing from it. Beyond the text it runs `git config` and
`git branch --show-current` in the directories the command names and `git --list-cmds`, and
stats paths to find a repository and the links inside it (`git_add_paths`). `shell_words` and
`shell_commands` follow bash's quoting, operators and simple commands. Each `cd` is followed in
order through `&&`, `||`, `!`, `;`, `&` and pipelines, so a forced add's paths are read against
every directory it could run in (and each `-C`). A git alias is expanded from the command's `-c`
and from `git config`, and a shell alias's value is checked as a command.

A forced add whose directory or paths the text cannot settle is refused:
- a relative path in a subshell, a command substitution, a script a shell reads from standard
  input, a `<(...)` or an --rcfile, `eval`, a function body, or a shell alias;
- an add run through a program that may change directory (`env -C`, `xargs`, `sudo`), with a
  variable or brace in the command word, subcommand or arguments, or written out in a single word
  another program may run (`sh -c`, `trap`, `script -c`, `flock -c`, `env -S`, a variable's
  value, a git `-c` value such as core.fsmonitor), or in text a shell gets only at run time
  (piped in, or built from variables), when that text spells the add out;
- one after a `cd` the text cannot follow: a variable, `cd -`, `popd`, a bare `pushd`, CDPATH or
  cdable_vars, a function definition, a shell alias, a loop followed by a `cd`, or text run in
  this shell (`${ cmd; }`, `eval`, `.`, `source`, a `trap`);
- one in a command that also makes a link, which the hook would stat before it exists; any add,
  forced or not, after a git config write that may move the work tree (`core.worktree`, an
  include, or a write the text cannot name the key of), and any alias after a write to it;
- a path `git_add_paths` cannot settle.
Any subcommand git does not know while help.autocorrect is on is refused, since git may run it
as another.

It cannot see a program that runs git itself (a script, `just`, a `git <name>` that git runs as
an external `git-<name>` program, `rebase --exec`, a script `.` or `source` reads from a file);
an add assembled at run time from text that does not spell it out; a push or commit piped into
a shell; a shell alias or function from the user's profile; plumbing that stages without `add`
(`update-index --add`, `apply --cached`); or a branch the command switches to before a push or
commit. Where GitHub has no server-side branch rules for the repository (AGENTS.md, Git workflow),
this hook is the only guard against direct pushes to main and next.
"""

import os
import re
import shlex
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402
import shell_words  # noqa: E402
from git_add_paths import (  # noqa: E402,F401  (repo_path: the tests read it here)
    GIT_MOVES, UNKNOWN, Refusal, check_add, forced, is_absolute, is_network, native, repo_path,
    tilde, unclear,
)
from shell_commands import (  # noqa: E402
    SHELLS, Command, all_words, commands, definition_body, leading, poison_index, shell_source,
    strip_reserved, walk,
)
from shell_words import Word  # noqa: E402

HOOK = "git-guard"
PROTECTED = {"main", "next"}
# Options of `git` itself that take a value, so the subcommand is found after them.
GIT_VALUE_OPTS = {
    "-C", "-c", "--git-dir", "--work-tree", "--namespace", "--config-env", "--attr-source",
    "--exec-path", "--super-prefix",
}
# Git 2.53's builtins (`git --list-cmds=builtins`), used when that call fails. Git runs a
# builtin even when an alias has its name, so none is looked up.
FALLBACK_BUILTINS = set("""
add am annotate apply archive backfill bisect blame branch bugreport bundle cat-file check-attr
check-ignore check-mailmap check-ref-format checkout checkout--worker checkout-index cherry
cherry-pick clean clone column commit commit-graph commit-tree config count-objects credential
credential-cache credential-cache--daemon credential-store describe diagnose diff diff-files
diff-index diff-pairs diff-tree difftool fast-export fast-import fetch fetch-pack fmt-merge-msg
for-each-ref for-each-repo format-patch fsck fsck-objects fsmonitor--daemon gc get-tar-commit-id
grep hash-object help hook index-pack init init-db interpret-trailers last-modified log ls-files
ls-remote ls-tree mailinfo mailsplit maintenance merge merge-base merge-file merge-index
merge-ours merge-recursive merge-recursive-ours merge-recursive-theirs merge-subtree merge-tree
mktag mktree multi-pack-index mv name-rev notes pack-objects pack-redundant pack-refs patch-id
pickaxe prune prune-packed pull push range-diff read-tree rebase receive-pack reflog refs remote
remote-ext remote-fd repack replace replay repo rerere reset restore rev-list rev-parse revert rm
send-pack shortlog show show-branch show-index show-ref sparse-checkout stage stash status
stripspace submodule--helper switch symbolic-ref tag unpack-file unpack-objects update-index
update-ref update-server-info upload-archive upload-archive--writer upload-pack var
verify-commit verify-pack verify-tag version whatchanged worktree write-tree
""".split())
# Programs that run the rest of their arguments as a command in the same directory.
SAME_DIRECTORY = {"env", "nice", "nohup", "timeout", "stdbuf", "ionice", "chrt", "taskset"}
# Programs whose arguments the guard does not read as commands: they print or test them.
NOT_RUNNERS = {"echo", "printf", "grep", "which", "type", "test", "["}
# Programs whose arguments are messages or search text the guard does not read as commands.
# Not sed or awk, which run commands (`sed e`, awk's system()).
MESSAGE_TAKERS = {"git", "gh", "rg", "cat", "head", "tail", "wc", "diff", "less"} | NOT_RUNNERS
# Variables that point git at other config, which may hold aliases the hook does not read.
CONFIG_HOMES = {"HOME", "XDG_CONFIG_HOME"}
# Programs that make a link the hook would have to stat before the command makes it.
LINKERS = {"ln", "mklink", "junction", "new-item"}
# `git config` options and subcommands that only read.
CONFIG_READS = {"--get", "--get-all", "--get-regexp", "--get-urlmatch", "--list", "-l",
                "--get-color", "--get-colorbool", "get", "list"}
CONFIG_WRITES = {"--add", "--unset", "--unset-all", "--replace-all"}
CONFIG_WHOLE = {"-e", "--edit", "--rename-section", "--remove-section"}
# Config keys that move the work tree or pull in other config.
MOVING_KEYS = ("core.worktree", "core.bare", "include.", "includeif.", "*")
CONFIG_FILE = re.compile(r"\.git[\\/]config|\.gitconfig|(^|[\\/])git[\\/]config")
AUTOCORRECT_OFF = ("0", "false", "never", "show", "no", "off")
MAX_STATES = 16
MAX_ALIASES = 64  # git follows long alias chains; past this many the guard refuses


def program(word):
    """The program a command word names, lowercased, without a path or a Windows extension."""
    name = re.split(r"[\\/]", word.text)[-1].lower()
    return re.sub(r"\.(exe|cmd|bat|com)$", "", name)


def git_form(word):
    """None, or the subcommand a git executable implies: "" for `git`, X for `git-X`."""
    name = program(word)
    if name == "git":
        return ""
    if name.startswith("git-") and len(name) > 4:
        return name[4:]
    return None


def is_expansion(word):
    """True when bash, not the text, decides the word: a variable, a brace or a glob."""
    return word.dynamic or word.split or (
        not word.quoted and re.search(r"[*?[]", word.text) is not None)


def forced_add_words(words):
    """True when the words, read with braces and commas as separators, hold `add` or `stage`
    and a force flag after it."""
    parts = [p.lower() for w in words for p in re.split(r"[\s,{}]+", w.text) if p]
    return any(p in ("add", "stage") and forced(parts[k + 1:]) for k, p in enumerate(parts))


def holds_forced_add(text):
    """True when `text`, as a command string some program may run, holds a forced git add."""
    parts = [Word(text=t) for t in re.split(r"[\s'\"`;|&()<>]+", text) if t]
    return any(git_form(p) is not None for p in parts) and forced_add_words(parts)


def known_commands():
    """The installed git's commands, which git runs before it looks for an alias: its builtins,
    its own programs, and `git-*` programs on PATH (`git --list-cmds=builtins,main,others`).
    Git 2.53's builtins when that call fails."""
    if not hasattr(known_commands, "cache"):
        try:
            out = subprocess.run(["git", "--list-cmds=builtins,main,others"],
                                 capture_output=True, text=True, timeout=5, check=False)
            names = set(out.stdout.split()) if out.returncode == 0 else set()
        except (OSError, subprocess.TimeoutExpired):
            names = set()
        known_commands.cache = names or FALLBACK_BUILTINS
    return known_commands.cache


def assigned_names(tokens):
    """Every variable the text may set: `NAME=`, `${NAME:=}`, `read`, `mapfile`, `printf -v`,
    and `export`, `declare` and the like."""
    names = set()
    for w in all_words(tokens):
        names |= set(re.findall(r"^([A-Za-z_]\w*)=", w.text))
        names |= set(re.findall(r"\$\{([A-Za-z_]\w*):?=", w.text))
    for c in walk(tokens):
        words = strip_reserved(c.words) or []
        head = words[0].text if words else ""
        if head in ("read", "mapfile", "readarray", "export", "declare", "typeset", "local",
                    "readonly") or head == "printf" and any(w.text == "-v" for w in words):
            names |= {m for w in words[1:] for m in re.findall(r"^([A-Za-z_]\w*)", w.text)}
    return names


def makes_links(tokens):
    """True when a command in the text may make a symlink or junction."""
    for c in walk(tokens):
        words = strip_reserved(c.words) or []
        if words and (program(words[0]) in LINKERS or program(words[0]) == "cp" and any(
                re.match(r"^(-[a-zA-Z]*s|--symbolic)", w.text) for w in words[1:])):
            return True
    return False


def written_config_keys(tokens):
    """The git config keys a command in the text may write, lowercased; "*" when it cannot
    say which: an editing option, a section renamed or removed, or a git config file named by a
    redirect or in the arguments of a program that may write it (`tee`, `cp`, `sed -i`)."""
    keys = set()
    for c in walk(tokens):
        words = strip_reserved(c.words) or []
        named = c.targets + (words[1:] if words and program(words[0]) not in MESSAGE_TAKERS
                             and git_form(words[0]) is None else [])
        if any(CONFIG_FILE.search(w.text) for w in named):
            keys.add("*")
        k = next((n for n, w in enumerate(words) if git_form(w) is not None), None)
        call = git_invocation(words[k:]) if k is not None else None
        if not call or call.sub.lower() != "config":
            continue
        texts = {a.text for a in call.args}
        values = [a for a in call.args if not a.text.startswith("-")]
        if values and values[0].text in ("set", "unset", "get", "list", "edit",
                                          "rename-section", "remove-section"):
            verb, values = values[0].text, values[1:]
        else:
            verb = None
        if verb in ("edit", "rename-section", "remove-section") or texts & CONFIG_WHOLE:
            keys.add("*")
        elif verb in ("set", "unset") or texts & CONFIG_WRITES or (
                verb is None and len(values) > 1 and not CONFIG_READS & texts):
            key = values[0] if values else Word(text="*")
            keys.add("*" if key.dynamic or key.split else key.text.lower())
    return keys


def indirect_words(tokens):
    """The text a program other than git gets, split into words: every command's words and
    here-document but those of commands git runs directly, which the guard checks itself."""
    texts = []
    for c in walk(tokens):
        words = strip_reserved(c.words) or []
        if words and git_form(words[0]) is not None:
            continue
        texts += [w.text for w in c.words] + ([c.stdin] if c.stdin else [])
    return [Word(text=t) for text in texts for t in re.split(r"[\s'\"`;|&()=]+", text) if t]


# --- following cd ----------------------------------------------------------------------------


def cd(state, head, args, assigned, cdable):
    """The directory after `cd` or `pushd` (`head`) with `args` from `state`. With `cdable`
    (bash's cdable_vars), a relative name may be a variable holding the directory."""
    options = [a.text for a in args if a.text.startswith("-") and a.text not in ("-", "--")]
    args = [a for a in args if a.text not in options]
    if head == "pushd" and "-n" in options:
        return state  # adds to the stack without changing directory
    if args and args[0].text == "--":
        args = args[1:]
    if not args and head == "pushd":
        return UNKNOWN  # swaps the top two directories of the stack
    if not args:
        return UNKNOWN if "HOME" in assigned else tilde(Word(text="~", tilde=True))
    target = args[0]
    path = tilde(target, assigned)
    if path is None or target.dynamic or target.split or path == "-" or re.match(r"^[+-]\d", path):
        return UNKNOWN
    path = native(path)
    if is_network(path):
        return UNKNOWN
    if is_absolute(path):
        return path
    if (os.environ.get("CDPATH") or "CDPATH" in assigned or cdable) and not re.match(
            r"^\.\.?(/|$)", path):
        return UNKNOWN
    return UNKNOWN if state is UNKNOWN else os.path.join(state, path)


def capped(states):
    return {UNKNOWN} if len(states) > MAX_STATES else states


class Checker:
    def __init__(self, cwd):
        self.cwd = cwd
        self.text = ""
        self.assigned = set()  # every variable the text may set
        self.lastpipe = False  # the text may turn on lastpipe, which keeps a pipeline's last cd
        self.cdable = False    # the text may turn on cdable_vars
        self.links = False     # the text may make a link before git runs
        self.config_keys = set()  # git config keys the text may write before git runs
        self.text_has_forced_add = False
        self.configs = {}

    def run(self, text):
        self.text = text
        tokens = shell_words.lex(text)[0]
        self.assigned = assigned_names(tokens)
        self.lastpipe = any(w.text == "lastpipe" for w in all_words(tokens))
        self.cdable = any(w.text == "cdable_vars" for w in all_words(tokens))
        self.links = makes_links(tokens)
        self.config_keys = written_config_keys(tokens)
        self.text_has_forced_add = forced_add_words(indirect_words(tokens))
        self.program(tokens, nested=False)

    @property
    def moved(self):
        return any(GIT_MOVES.match(name) for name in self.assigned)

    def program(self, tokens, nested):
        """Check every command of `tokens`. Nested commands (a subshell, a substitution, a
        command string) start where the text cannot settle."""
        cmds = commands(tokens)
        poison = poison_index(cmds)
        list_in = {UNKNOWN} if nested else {self.cwd}
        i = 0
        while i < len(cmds):
            i, ok, fail, joiner = self.and_or(cmds, i, list_in, poison)
            if joiner != "&":  # a backgrounded list runs in a subshell
                list_in = capped(ok | fail)

    def and_or(self, cmds, i, list_in, poison):
        """Check one list of pipelines joined by && and ||, from `cmds[i]`: (the index after
        it, states on success, states on failure, the operator that ends it)."""
        ok = fail = joiner = None
        while i < len(cmds):
            inp = list_in if joiner is None else (ok if joiner == "&&" else fail)
            i, p_ok, p_fail = self.pipeline(cmds, i, inp, poison)
            if joiner is None:
                ok, fail = p_ok, p_fail
            elif joiner == "&&":
                ok, fail = p_ok, capped(fail | p_fail)
            else:
                ok, fail = capped(ok | p_ok), p_fail
            joiner = cmds[i - 1].conn
            if joiner not in ("&&", "||"):
                break
        return i, ok, fail, joiner

    def pipeline(self, cmds, i, inp, poison):
        """Check the pipeline starting at `cmds[i]`: (the index after it, states on success,
        states on failure)."""
        j = i
        while cmds[j].conn in ("|", "|&") and j + 1 < len(cmds):
            j += 1
        for k in range(i, j + 1):
            cmds[k].piped = k > i
            ok, fail = self.command(cmds[k], {UNKNOWN} if k >= poison else inp)
        if j > i:  # bash runs each element of a pipeline in a subshell, the last too
            ok, fail = (capped(inp | ok), capped(inp | fail)) if self.lastpipe else (inp, inp)
        return j + 1, ok, fail

    def command(self, c, states):
        """(states on success, states on failure) after one simple command."""
        lead = leading(c.words)
        ok, fail = self.simple(c, states)
        if "coproc" in lead:
            ok = fail = states
        if lead.count("!") % 2:
            ok, fail = fail, ok
        return ok, fail

    def simple(self, c, states):
        for inner in c.nested:
            self.program(inner, nested=True)
        body = definition_body(c.words)
        if body is not None:  # a function's body, checked once whatever the function is named
            inner = Command()
            inner.words = body
            self.simple(inner, {UNKNOWN})
            return states, states
        words = strip_reserved(c.words)
        self.command_strings(words)
        if any("$((" in w.text or "@P}" in w.text for w in c.words):
            # bash evaluates a variable's value as arithmetic, whose subscripts run commands
            self.runtime_text("in a variable bash evaluates")
        if not words:
            return states, states
        head = words[0]
        if head.text in ("cd", "pushd"):
            return capped({cd(s, head.text, words[1:], self.assigned, self.cdable)
                           for s in states}), states
        if head.text == "popd":
            return {UNKNOWN}, states
        if head.text == "alias":
            self.shell_alias(words[1:])
        if git_form(head) is not None:
            self.git(words, states)
            return states, states
        if is_expansion(head) and (forced_add_words(words) or any(
                w.text.lower() in ("add", "stage") for w in words[1:])
                and any(is_expansion(w) for w in words[1:])):
            raise unclear(f"run through {head.text}")
        if is_expansion(head):  # the command may be text set elsewhere, as by read or mapfile
            self.runtime_text(f"run through {head.text}")
        self.shell_runs(c, words)
        self.wrapped(c, words, states)
        return states, states

    def command_strings(self, words):
        """Refuse a forced git add written out in a single word that a program may run as a
        command: `trap '...'`, `script -c '...'`. Words given to git, gh, echo and the like are
        messages; a variable's value reaches runtime_text where it is run."""
        if not words or program(words[0]) in MESSAGE_TAKERS or git_form(words[0]) is not None:
            return
        for text in (w.text for w in words[1:]):
            if holds_forced_add(text):
                raise unclear("written in a string a program may run as a command",
                              "run git directly in this shell")

    def shell_runs(self, c, words):
        """Check what a shell, `.` or `source` runs: a `-c` string, its standard input, or a
        `<(...)`; a script file is out of reach."""
        head = program(words[0])
        if head in (".", "source"):
            source = shell_source([Word(text="--")] + words[1:])
        elif head in SHELLS:
            source = shell_source(words[1:])
        else:
            return
        if {"BASH_ENV", "ENV"} & self.assigned:
            self.runtime_text("in a startup file the shell reads")
        kind, word = source
        if kind == "stdin" and c.stdin_sub:
            kind = "substitution"
        if kind == "string":  # a variable in it reaches runtime_text as a command word
            self.program(shell_words.lex(word.text)[0], nested=True)
        elif kind == "substitution":
            self.runtime_text("in a script a shell reads from a substitution")
        elif kind == "stdin" and c.stdin is not None:  # a variable in it reaches runtime_text
            self.program(shell_words.lex(c.stdin)[0], nested=True)
        elif kind == "stdin" and c.piped:
            self.runtime_text("piped into a shell")

    def shell_alias(self, args):
        """Check a shell alias's value as a command, and refuse one that runs bare git or a git
        alias, which the hook could not follow where the alias is used."""
        for a in args:
            name, eq, value = a.text.partition("=")
            if not eq:
                continue
            self.program(shell_words.lex(value)[0], nested=True)
            words = [w for kind, w in shell_words.lex(value)[0] if kind == "word"]
            k = next((n for n, w in enumerate(words) if git_form(w) is not None), None)
            if k is None:
                continue
            call = git_invocation(words[k:])
            if call is None or call.sub not in known_commands():
                raise Refusal(f"the shell alias {name} runs git in a way the hook cannot check; "
                              "run git directly")

    def runtime_text(self, how):
        """Refuse when the command text holds a forced add that a shell gets only at run time."""
        if self.text_has_forced_add:
            raise unclear(how, "run git directly in this shell")

    def wrapped(self, c, words, states):
        """Check `eval`'s string, and look for git or a shell among a command's arguments, as
        `env`, `xargs`, `sudo` and `find -exec` run them."""
        head = program(words[0])
        if head in NOT_RUNNERS:
            return
        if head == "eval":  # a variable in it reaches runtime_text as a command word
            self.program(shell_words.lex(" ".join(w.text for w in words[1:]))[0], nested=True)
        for j in range(1, len(words)):
            if git_form(words[j]) is not None:
                same = head in SAME_DIRECTORY and not changes_directory(head, words[1:j])
                self.git(words[j:], states if same else {UNKNOWN},
                         through=None if same else words[0].text)
                return
            if program(words[j]) in SHELLS:
                self.shell_runs(c, words[j:])
                self.wrapped(c, words[j:], states)
                return

    def git(self, words, states, through=None):
        call = self.expand(git_invocation(words), states)
        if call is None:
            return
        if is_expansion(call.sub_word):
            raise unclear(f"with the subcommand {call.sub}, which the text cannot settle",
                          "name the subcommand")
        sub = call.sub.lower()
        wheres = {where(s, call.dirs, self.assigned) for s in states}
        if sub in ("add", "stage"):
            self.check_git_add(call, wheres, through)
        elif sub == "push":
            for w in wheres:
                reason = check_push([a.text for a in call.args],
                                    hookio.current_branch(w or self.cwd))
                if reason:
                    raise Refusal(reason)
        elif sub == "commit":
            for w in wheres:
                branch = hookio.current_branch(w or self.cwd)
                if branch in PROTECTED:
                    raise Refusal(f"commit while on {branch}; branch from origin/next first")

    def check_git_add(self, call, wheres, through):
        may_force = forced([a.text for a in call.args]) or any(
            a.dynamic or a.split for a in call.args)
        if through and may_force:
            raise unclear(f"run through {through}")
        if may_force and self.links:
            raise unclear("in a command that also makes a link, which the hook would stat "
                          "before it exists", "make the link in a separate command")
        moved = call.moved or self.moved or any(
            k.startswith(MOVING_KEYS) for k in self.config_keys)
        check_add(call.args, wheres, moved, self.assigned)

    def expand(self, call, states):
        """The call with its git aliases expanded, or None when it runs no git subcommand the
        guard checks. Refuses a forced add written in a `-c` value git may run (a hook or
        fsmonitor command)."""
        for _ in range(MAX_ALIASES):
            if call is None:
                return None
            if any(holds_forced_add(v) for v in call.config.values()):
                raise unclear("in a git -c value git may run", "run git add itself")
            if call.sub.lower() in ("add", "stage", "push", "commit") or call.dashed:
                return call
            expansion = self.alias(call, states)
            if expansion is None:
                return call
            if expansion.startswith("!"):
                text = expansion[1:] + " " + " ".join(shlex.quote(a.text) for a in call.args)
                self.program(shell_words.lex(text)[0], nested=True)
                return None
            try:
                expanded = shlex.split(expansion)
            except ValueError:
                raise unclear(f"behind the alias {call.sub}, whose value cannot be split",
                              "run `git add` itself") from None
            call = git_invocation([Word(text="git")] + call.prefix
                                  + [Word(text=t) for t in expanded] + call.args)
        raise unclear(f"behind a chain of more than {MAX_ALIASES} aliases", "run `git add` itself")

    def alias(self, call, states):
        """The alias the subcommand names, or None. Refuses when the text sets it in a way the
        hook cannot read, or when help.autocorrect may run an unknown subcommand as another."""
        sub = call.sub
        if not sub or sub.startswith("-") or is_expansion(call.sub_word) or sub in known_commands():
            return None
        key = "alias." + sub.lower()
        if key in call.config:
            return call.config[key]
        includes = [k for k in list(call.config) + list(call.config_env)
                    if k.startswith(("include.", "includeif."))]
        written = any(k == key or k.startswith(("help.autocorrect", "include", "*"))
                      for k in self.config_keys)
        if (key in call.config_env or includes or self.moved or written
                or CONFIG_HOMES & self.assigned):
            raise unclear(f"behind {sub}, from git config the command sets",
                          "run `git add` itself")
        autocorrect = "on" if "help.autocorrect" in call.config_env else call.config.get(
            "help.autocorrect")
        for state in states:
            config = self.config(where(state, call.dirs, self.assigned))
            if config is None:
                raise unclear(f"behind {sub}, whose git config could not be read",
                              "run `git add` itself")
            if key in config:
                return config[key]
            value = (autocorrect or config.get("help.autocorrect", "0")).lower()
            if value not in AUTOCORRECT_OFF:
                raise Refusal(f"`git {sub}` is not a git command or alias, and help.autocorrect "
                              "may run it as another, which the hook cannot check; spell the "
                              "command out")
        return None

    def config(self, directory):
        """The alias.* and help.autocorrect settings git reads in `directory`, or None when
        `git config` fails. Reading config runs no hook, filter or alias."""
        directory = directory if directory and os.path.isdir(directory) else self.cwd
        if directory not in self.configs:
            try:
                out = subprocess.run(
                    ["git", "-C", directory, "config", "-z", "--get-regexp",
                     r"^(alias\..*|help\.autocorrect)$"],
                    capture_output=True, text=True, timeout=5, check=False)
            except (OSError, subprocess.TimeoutExpired):
                return None
            if out.returncode not in (0, 1):
                return None
            entries = (e.partition("\n") for e in out.stdout.split("\0") if e)
            self.configs[directory] = {k.lower(): v for k, _, v in entries}
        return self.configs[directory]


def changes_directory(head, args):
    """True when `env` is told to run its command elsewhere (-C, --chdir, or a prefix of it)
    or to split a string into one (-S, --split-string)."""
    if head != "env":
        return False
    for a in args:
        name = a.text.split("=")[0]
        if re.match(r"^-[a-zA-Z]*[CS]", name) or len(name) >= 3 and (
                "--chdir".startswith(name) or "--split-string".startswith(name)):
            return True
    return False


# --- git's own arguments ---------------------------------------------------------------------


class GitCall:
    def __init__(self):
        self.prefix, self.dirs, self.config, self.config_env = [], [], {}, set()
        self.moved = False  # --git-dir, --work-tree or core.worktree
        self.sub, self.sub_word, self.args, self.dashed = "", Word(), [], False


def git_invocation(words):
    """The git call in `words` (starting at the git word), or None when it names no subcommand."""
    call = GitCall()
    call.sub = git_form(words[0])
    call.dashed = bool(call.sub)
    i = 1
    while not call.dashed and i < len(words) and words[i].text.startswith("-"):
        i = git_option(call, words, i)
    if not call.dashed:
        if i >= len(words):
            return None
        call.sub, call.sub_word = words[i].text, words[i]
        i += 1
    call.args = words[i:]
    return call


def git_option(call, words, i):
    """Record git's own option at `words[i]` in `call`; the index after it."""
    opt = words[i].text
    name, eq, attached = opt.partition("=")
    value = attached if eq else (words[i + 1].text if i + 1 < len(words) else "")
    takes = opt in GIT_VALUE_OPTS
    if name in ("--git-dir", "--work-tree"):
        call.moved = True
    if opt == "-C":  # git applies each -C in turn: `-C a -C b` is a/b
        call.dirs.append(words[i + 1] if i + 1 < len(words) else Word())
    if opt == "-c":
        key, _, val = value.partition("=")
        call.config[key.lower()] = val
        call.moved |= key.lower() == "core.worktree"
    if name == "--config-env":
        key = value.partition("=")[0].lower()
        call.config_env.add(key)
        call.moved |= key == "core.worktree"
    call.prefix += words[i:i + (2 if takes else 1)]
    return i + (2 if takes else 1)


def where(state, dirs, assigned=frozenset()):
    """The directory git runs in: `state` with each `-C` applied in turn."""
    for d in dirs:
        path = tilde(d, assigned)
        if path is None or d.dynamic or d.split or is_network(path):
            return UNKNOWN
        path = native(path)
        if is_absolute(path):
            state = path
        elif state is not UNKNOWN:
            state = os.path.join(state, path)
    return state


def ref_name(ref):
    ref = ref.lstrip("+")
    return ref[len("refs/heads/"):] if ref.startswith("refs/heads/") else ref


def check_push(args, branch):
    flags = [a for a in args if a.startswith("-")]
    positional = [a for a in args if not a.startswith("-")]
    lease = any(f.startswith("--force-with-lease") for f in flags)
    forced_push = any(f in ("-f", "--force") for f in flags) or any(
        p.startswith("+") for p in positional[1:]
    )
    if forced_push and not lease:
        return "force push without --force-with-lease"
    if "--all" in flags or "--mirror" in flags:
        return "`git push --all` or `--mirror` would push main and next"
    refspecs = positional[1:]
    if not refspecs:
        if branch in PROTECTED:
            return f"bare `git push` while on {branch}; open a PR instead"
        return None
    for spec in refspecs:
        src, _, dst = spec.partition(":")
        target = ref_name(dst if dst else src)
        if target == "HEAD":
            target = branch
        if target in PROTECTED:
            return f"push to {target}; changes reach main and next only through a PR"
    return None


def main():
    try:
        data = hookio.read_input()
        command = hookio.tool_input(data).get("command", "") or ""
        if not isinstance(command, str):
            raise ValueError("tool_input.command is not a string")
    except ValueError:
        hookio.block(HOOK, "could not parse the hook input, so the command was not checked")
    cwd = data.get("cwd") or os.getcwd()
    if not isinstance(cwd, str):
        raise TypeError(f"cwd is a {type(cwd).__name__}, not a path")
    try:
        Checker(native(cwd)).run(command)
    except shell_words.LexError as error:
        if re.search(r"\bgit", command):
            hookio.block(HOOK, f"could not read the command ({error}), so its git call was not "
                         "checked")
    except Refusal as refusal:
        hookio.block(HOOK, str(refusal))
    sys.exit(0)


if __name__ == "__main__":
    hookio.run_guard(HOOK, main)
