# SPDX-License-Identifier: AGPL-3.0-or-later
"""git-guard's reading of the simple commands in a lexed bash command line.

`commands` groups `shell_words` tokens into simple commands, each with the operator after it, its
here-document or here-string text, and the token lists of its subshells and substitutions. The
other helpers say what bash makes of a command's leading words and of a shell's options.
"""

import re

import shell_words
from shell_words import Word

SHELLS = {"sh", "bash", "dash", "zsh", "ksh", "mksh", "ash", "fish"}
# Leading words after which the rest is still a command: reserved words and `time`. `!` also
# inverts the command's status; `coproc` runs it in a subshell.
RESERVED = {"!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until",
            "esac", "time", "coproc"}
LOOPS = {"while", "until", "for", "select"}
# Shell options that take a value, so the word after them is not the script.
SHELL_VALUE_OPTS = {"-o", "+o", "-O", "+O", "--rcfile", "--init-file"}
# Script paths that are the shell's own standard input.
STDIN_FILES = {"/dev/stdin", "/dev/fd/0", "/proc/self/fd/0", "-"}


class Command:
    def __init__(self):
        self.words, self.nested, self.conn = [], [], None
        self.stdin = None           # the text of a here-document or here-string
        self.stdin_sub = False      # standard input comes from a `<(...)`
        self.piped = False          # reads the output of the command before it in a pipeline
        self.targets = []           # the words its redirections name, here-documents aside


def commands(tokens):
    """The simple commands of a token list, each with the operator after it. A subshell's
    tokens, and every substitution's, go in the command's `nested`."""
    out, cur, i, cases = [], Command(), 0, 0
    while i < len(tokens):
        kind, value = tokens[i]
        if kind == "word":
            cases += starts(cur, value, "case") - starts(cur, value, "esac")
            cur.words.append(value)
            cur.nested += value.nested
        elif kind == "redir":
            i += redirect(cur, value, tokens[i + 1:i + 2])
        elif kind == "nested":
            cur.nested.append(value)
        elif value == "(" and cases and (not cur.words or cur.words[-1].text == "in"):
            pass  # the optional `(` before a case pattern
        elif value == "(" and len(cur.words) == 1 and tokens[i + 1:i + 2] == [("op", ")")]:
            cur.words.append(Word(text="()", definition=True))
            i += 1
        elif value == "(":
            i = subshell(cur, tokens, i)
        else:
            cur.conn = value
            out.append(cur)
            cur = Command()
        i += 1
    if cur.words or cur.nested:
        out.append(cur)
    return out


def starts(cur, word, keyword):
    """1 when `word` is the unquoted `keyword` at the start of a command."""
    return int(not word.quoted and word.text == keyword
               and all(w.text in RESERVED for w in cur.words))


def redirect(cur, op, target):
    """Record a redirection's target in `cur`; the number of tokens it used after `op`."""
    if not target or target[0][0] != "word":
        return 0
    word = target[0][1]
    cur.nested += word.nested
    if op not in ("<<", "<<-", "<<<"):
        cur.targets.append(word)
    if op in ("<<", "<<-"):
        cur.stdin = word.body or ""
    elif op == "<<<":
        cur.stdin = word.text
    elif op == "<" and word.text.startswith("<(") and word.dynamic:
        cur.stdin_sub = True
    return 1


def subshell(cur, tokens, i):
    """Put the tokens of the `( ... )` opening at `i` in `cur.nested`; the index of its `)`. A
    `)` that ends a `case` pattern inside does not close it."""
    j, depth, cases = i + 1, 1, 0
    while j < len(tokens) and depth:
        kind, value = tokens[j]
        if kind == "word" and not value.quoted and tokens[j - 1][0] == "op":
            cases += {"case": 1, "esac": -1}.get(value.text, 0)
        elif kind == "op" and not (value == ")" and depth == 1 and cases):
            depth += {"(": 1, ")": -1}.get(value, 0)
        j += 1
    if depth:
        raise shell_words.LexError("unterminated (")
    cur.nested.append(tokens[i + 1:j - 1])
    return j - 1


def strip_reserved(words):
    """The words after leading reserved words and `command`/`builtin`; None for a command whose
    words are not commands (`for x in ...`, `case`, `command -v`). A function definition keeps
    its name first, so its body is read as the arguments of another program."""
    words = list(words)
    while words:
        head = words[0].text
        reserved = not words[0].quoted  # a quoted reserved word is an ordinary word
        if reserved and head in ("for", "select", "case"):
            return None
        if reserved and head in RESERVED or head in ("command", "builtin"):
            words.pop(0)
            while words and words[0].text.startswith("-") and head in ("time", "command"):
                if head == "command" and re.search(r"[vV]", words[0].text):
                    return None
                words.pop(0)
            continue
        break
    while words and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[0].text):
        words.pop(0)  # leading VAR=value assignments
    return words


def leading(words):
    """The reserved words and loop keywords a command starts with."""
    out = []
    for w in words:
        if w.quoted or w.text not in RESERVED | LOOPS | {"function"}:
            break
        out.append(w.text)
    return out


def poison_index(cmds):
    """The first command from which the directory is unknown: a loop followed by a `cd` (the
    loop's body repeats), a function definition (its body runs when called), a shell alias (its
    value runs wherever its name is used), or text run in this shell, where its `cd` sticks
    (`${ cmd; }`, `eval`, `.`, `source`, a `trap`)."""
    poison = len(cmds)
    for i, c in enumerate(cmds):
        head = (strip_reserved(c.words) or [Word()])[0]
        if ("function" in leading(c.words) or any(w.definition or w.in_shell for w in c.words)
                or head.text == "alias" and any("=" in w.text for w in c.words)
                or head.text in ("eval", ".", "source", "trap")):
            poison = min(poison, i)
        if LOOPS & set(leading(c.words)) and any(
                (strip_reserved(d.words) or [Word()])[0].text in ("cd", "pushd", "popd")
                for d in cmds[i:]):
            poison = min(poison, i)
    return poison


def all_words(tokens):
    for kind, value in tokens:
        if kind == "word":
            yield value
            for inner in value.nested:
                yield from all_words(inner)
        elif kind == "nested":
            yield from all_words(value)


def definition_body(words):
    """The words after a function's `NAME ()` or `function NAME`, or None when `words` define
    no function. Only the body's first command is in them; the rest follow as commands."""
    k = next((n for n, w in enumerate(words) if w.definition), None)
    if k is not None:
        return words[k + 1:]
    lead = leading(words)
    if "function" in lead:
        body = words[lead.index("function") + 2:]
        return body[1:] if body and body[0].definition else body
    return None


def walk(tokens):
    """Every simple command in `tokens`, in subshells and substitutions too."""
    for c in commands(tokens):
        yield c
        for inner in c.nested:
            yield from walk(inner)


def shell_source(args):
    """What a shell with these arguments runs: ("string", word) for `-c`, ("stdin", None),
    ("script", word) for a file, or ("substitution", word) for a `<(...)` it reads."""
    k = 0
    while k < len(args):
        a = args[k].text
        if a in SHELL_VALUE_OPTS:
            value = args[k + 1] if k + 1 < len(args) else Word()
            if value.text.startswith(("<(", ">(")) and value.dynamic:
                return ("substitution", value)  # an --rcfile or --init-file bash sources
            k += 2
            continue
        if a == "--":
            return script(args[k + 1]) if k + 1 < len(args) else ("stdin", None)
        if a.startswith("-") and not a.startswith("--") and "s" in a and "c" not in a:
            return ("stdin", None)  # -s reads commands from stdin; the rest are arguments
        if a.startswith("-") and not a.startswith("--") and "c" in a:
            rest = args[k + 1:]
            while rest and rest[0].text.startswith(("-", "+")):
                rest = rest[2:] if rest[0].text in SHELL_VALUE_OPTS else rest[1:]
            return ("string", rest[0]) if rest else ("stdin", None)
        if not a.startswith(("-", "+")):
            return script(args[k])
        k += 1
    return ("stdin", None)


def script(word):
    if word.text in STDIN_FILES:
        return ("stdin", None)
    if word.text.startswith(("<(", ">(")) and word.dynamic:
        return ("substitution", word)
    return ("script", word)
