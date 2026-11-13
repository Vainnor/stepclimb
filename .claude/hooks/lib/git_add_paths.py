# SPDX-License-Identifier: AGPL-3.0-or-later
"""git-guard's check of the paths a forced `git add` names, from the text alone.

A path is read against the directory the add runs in, up to the repository top: `\\` as a
separator, as on Windows, a leading `:/` as the top, and case ignored, as macOS and Windows file
systems ignore it. A path the text cannot settle is refused rather than guessed: pathspec
magic, a glob, a variable, a brace, `-A` with no path, a pathspec file, a path outside the
repository, a symlink or junction inside it, a backslash off Windows (git reads it as a glob
escape there), and a name that could be another name for `references` or `private` (an 8.3 short
name such as `REFERE~1`, a trailing dot or space, a non-ASCII name that folds to one, a
drive-relative `C:x`). Checking for a link runs lstat on paths inside the repository only.
"""

import os
import posixpath
import re
import stat
import unicodedata

import hookio

UNKNOWN = None  # a directory the text cannot settle
LOCAL_ONLY = ("references", "private")
# Files under references/ and private/ that .gitignore keeps tracked; content-guard allows them too.
# Lowercase, as repo_path returns; the file name must also match in case, as git's ignore rules
# do on Linux.
TRACKED_LOCAL_ONLY = {"references/readme.md", "references/index.md", "private/readme.md"}
TRACKED_NAMES = {"README.md", "INDEX.md"}
# Variables whose value moves git's work tree or git directory, or adds config that may.
GIT_MOVES = re.compile(r"^GIT_(DIR|WORK_TREE|CONFIG|CONFIG_PARAMETERS|CONFIG_COUNT|CONFIG_GLOBAL"
                       r"|CONFIG_SYSTEM|CONFIG_(KEY|VALUE)_\d+)$")


class Refusal(Exception):
    """A reason to block the command."""


def unclear(what, hint="name the paths and run git directly in this shell"):
    return Refusal(f"a forced `git add` {what} cannot be checked against references/ and "
                   f"private/; {hint} (.claude/rules/content-and-ip.md)")


def native(path):
    """`path` as git.exe receives it: on Windows, Git Bash turns `/c/...` into `c:/...`."""
    return re.sub(r"^/([a-zA-Z])/", r"\1:/", path) if os.name == "nt" else path


def is_absolute(path):
    path = native(path)
    return path.startswith(("/", "\\")) or (
        os.name == "nt" and re.match(r"^[a-zA-Z]:[\\/]", path) is not None)


def is_network(path):
    """A UNC path (`//host/share`), whose stat could wait on the network."""
    return native(path).replace("\\", "/").startswith("//")


def tilde(word, assigned=frozenset()):
    """The word's text with bash's tilde expansion, or None when the text cannot say, as when
    it sets HOME itself."""
    if not word.tilde:
        return word.text
    head, sep, rest = word.text.partition("/")
    if head == "~":
        if "HOME" in assigned:
            return None
        return native(os.environ.get("HOME") or os.path.expanduser("~")) + sep + rest
    if head in ("~+", "~-") or not re.match(r"^~[A-Za-z0-9._-]+$", head):
        return None
    expanded = os.path.expanduser(head)
    return None if expanded == head else expanded + sep + rest


def forced(args):
    """True when the `git add` arguments include -f, in a cluster (-Af) or as any prefix of
    --force that git accepts (--fo)."""
    for a in args:
        if a == "--":
            return False
        if re.match(r"^-[a-zA-Z]*f", a) or (len(a) >= 3 and "--force".startswith(a)):
            return True
    return False


def may_be_option(word):
    """True when bash may turn the word into an option such as -f: a variable or substitution,
    or a brace whose text starts with or offers one (`{a,-f}`)."""
    return word.dynamic or word.split and re.match(r"^(-|\{([^{}]*,)?-)", word.text) is not None


def repo_path(path, where):
    """`path` relative to the repository top holding `where`, `/`-separated and lowercased (macOS
    and Windows file systems ignore case), starting with `..` when it lies outside the
    repository. A backslash counts as a separator, as on Windows, and a leading `:/` names the
    top."""

    def slashed(p):
        # Rooted at `/` so posixpath treats a Windows `c:/...` path as absolute too.
        p = p.replace("\\", "/").lower()
        return p if p.startswith("/") else "/" + p

    path = native(path).replace("\\", "/").lower()
    top = slashed(hookio.repo_root(where) or where)
    base = slashed(where)
    if path.startswith(":/"):
        path, base = path[2:], top
    if os.name == "nt" and re.match(r"^[a-z]:/", path):  # a drive path is absolute there
        path = "/" + path
    return posixpath.relpath(posixpath.normpath(posixpath.join(base, path)), top)


def stages_all(option):
    """-A, --all, --no-ignore-removal, a cluster holding A, or a prefix git accepts."""
    return bool(re.match(r"^-[a-zA-Z]*A", option)) or (
        len(option) >= 3 and "--all".startswith(option)
    ) or (len(option) >= 5 and "--no-ignore-removal".startswith(option))


def add_arguments(args):
    """(options, paths) of a `git add`; after `--`, all are paths."""
    options, paths = [], []
    for k, a in enumerate(args):
        if a.text == "--":
            paths += args[k + 1:]
            break
        (options if a.text.startswith("-") else paths).append(a)
    return options, paths


def check_add(args, wheres, moved, assigned):
    """Refuse a `git add` with `args`, run in each of `wheres`, that may stage a local-only
    path or whose paths the text cannot settle. `moved` is a work tree or git directory the
    command moves."""
    if moved:
        raise Refusal("a `git add` with its work tree or git directory moved (--work-tree, "
                      "--git-dir, core.worktree, GIT_DIR, GIT_WORK_TREE, GIT_CONFIG, "
                      "GIT_CONFIG_PARAMETERS, GIT_CONFIG_COUNT, GIT_CONFIG_KEY_n, "
                      "GIT_CONFIG_VALUE_n, GIT_CONFIG_GLOBAL, GIT_CONFIG_SYSTEM) cannot be "
                      "checked against references/ and private/; run it without them")
    expansion = next((a.text for a in args if a.dynamic or a.split), None)
    if not forced([a.text for a in args]):
        before = args[:next((k for k, a in enumerate(args) if a.text == "--"), len(args))]
        if not any(may_be_option(a) for a in before):
            return  # after `--` an expansion can only name paths
        # A variable or brace may expand to -f, or to -Af.
        raise Refusal(f"a `git add` with {expansion}, which may expand to -f, cannot be "
                      "checked against references/ and private/; name the paths "
                      "(.claude/rules/content-and-ip.md)")
    options, paths = add_arguments(args)
    vague = vague_paths(paths, assigned)
    if any(len(o.text) >= 4 and "--pathspec-from-file".startswith(o.text.split("=")[0])
           for o in options):
        vague.append("--pathspec-from-file")
    if not paths and any(stages_all(o.text) for o in options):
        vague.append("-A")
    if expansion is not None:  # bash expands it before git runs
        vague.append(expansion)
    if vague:
        raise unclear(f"of {vague[0]}")
    for w in wheres:
        for p in paths:
            check_path(tilde(p, assigned), w)


def vague_paths(paths, assigned):
    """The paths whose meaning git or bash decides, not the text."""
    texts = [p.text for p in paths]
    # Pathspec magic other than a plain `:/`; git reads short magic after `:/` too (`:/!x`,
    # `:/^x`, `:/:`).
    vague = [t for t in texts if t.startswith(":")
             and (not t.startswith(":/") or t[2:3] in ("!", "^", ":"))]
    vague += [t for t in texts if re.search(r"[*?[]", t)]
    if os.name != "nt":  # git reads `\` as a glob escape off Windows: `reference\s` is references
        vague += [t for t in texts if "\\" in t]
    vague += [p.text for p in paths if tilde(p, assigned) is None]
    return vague


def windows_alias(first):
    """True when the first component of a repository path could name references/ or private/
    by another name: an 8.3 short name (REFERE~1, or a hashed one such as RE1A2B~1), a trailing
    dot or space, which Windows drops, or a non-ASCII name whose case-insensitive form is one
    of them (`referenceſ`), as NTFS and APFS may compare it."""
    if re.match(r"^(re|pr)[^~./]{0,4}~\d", first):
        return True
    if first != first.rstrip(". ") and first.rstrip(". ") in LOCAL_ONLY:
        return True
    folded = unicodedata.normalize("NFKC", first).upper().lower()
    return not first.isascii() and folded.rstrip(". ") in LOCAL_ONLY


def check_path(path, where):
    if where is UNKNOWN and not is_absolute(path):
        raise unclear(f"of {path} after a `cd` the hook cannot follow, or in a subshell",
                      "run git with -C instead")
    if re.match(r"^[a-zA-Z]:(?![\\/])", path):
        raise unclear(f"of the drive-relative path {path}")
    if is_network(path):
        raise unclear(f"of the network path {path}")
    base = native(path) if where is UNKNOWN else where
    top = hookio.repo_root(base)
    if top is None:
        raise unclear(f"of {path} from {base}, outside any repository")
    rel = repo_path(path, base)
    if rel == ".." or rel.startswith("../"):
        raise unclear(f"of {path}, outside the repository from where it runs",
                      "name a path inside it")
    first = rel.split("/")[0]
    if windows_alias(first):
        raise unclear(f"of {path}, through {first}, which Windows may read as another name")
    if path.startswith(":/"):
        target = os.path.join(top, native(path)[2:])
    else:
        target = native(path) if where is UNKNOWN else os.path.join(where, native(path))
    # Git adds a symlink named last as a link. Whether Git for Windows reads a junction named
    # last as a folder is not checked, so there the last component counts too.
    link = symlink(target, top, leaf=os.name == "nt")
    if link:
        raise unclear(f"of {path}, through the link {link}")
    name = path.replace("\\", "/").split("/")[-1]
    if rel in TRACKED_LOCAL_ONLY and name in TRACKED_NAMES:
        return
    if rel == "." or first in LOCAL_ONLY:
        raise Refusal(f"`git add --force {path}` would stage local-only references or private data "
                      "(.claude/rules/content-and-ip.md)")


def is_link(path):
    """True for a symlink, or on Windows any reparse point (a junction too). One lstat."""
    try:
        st = os.lstat(path)
    except OSError:
        return False
    if stat.S_ISLNK(st.st_mode):
        return True
    reparse = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    return bool(getattr(st, "st_file_attributes", 0) & reparse)


def symlink(path, top, leaf):
    """The first symlink or junction inside `top` that `path` passes through, before any `..`
    folds it away; the last component counts only with `leaf`. Only paths inside `top` are
    stat'ed."""
    parts = re.split(r"[\\/]", native(path))
    anchor = parts[0] + os.sep if parts[0] else os.sep
    inside = os.path.normcase(os.path.abspath(top)).rstrip("\\/") + os.sep
    if not leaf and parts[-1] not in ("", ".", ".."):
        parts = parts[:-1]
    stack = []
    for name in (p for p in parts[1:] if p not in ("", ".")):
        if name == "..":
            if stack:
                stack.pop()
            continue
        stack.append(name)
        current = os.path.join(anchor, *stack)
        if os.path.normcase(current).startswith(inside) and is_link(current):
            return current
    return None
