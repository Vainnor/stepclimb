# SPDX-License-Identifier: AGPL-3.0-or-later
"""Read a bash command line into words and operators without running any of it.

git-guard reads a Bash tool command before it runs. This lexer follows bash's quoting (`'...'`,
`"..."`, `$'...'`, backslashes) and keeps redirections and the control operators (`&&`, `||`,
`;`, `&`, `|`, newline, parentheses) apart from words. It keeps each here-document's body with
its delimiter word, as data, and lexes the text of every command substitution (`$(...)`,
backticks, `<(...)`, `>(...)`, and those inside `${...}`, `$((...))` and an unquoted
here-document) so the guard can look inside it. Parameter and arithmetic expansions are left as
written and flagged: their values are not in the text.
"""

import re
from dataclasses import dataclass, field


class LexError(ValueError):
    """The text is not a complete bash command: an unterminated quote or substitution."""


@dataclass
class Word:
    text: str = ""          # after quote removal; expansions kept as written
    dynamic: bool = False   # holds a parameter, command or arithmetic expansion
    quoted: bool = False    # any part was quoted or escaped
    tilde: bool = False     # starts with an unquoted ~, which bash expands
    split: bool = False     # holds a brace expansion such as {a,b}, which bash turns into
                            # several words
    nested: list = field(default_factory=list)  # token lists of its command substitutions
    body: str = None        # for a here-document's delimiter word, the document's text
    in_shell: bool = False  # holds a `${ cmd; }` that runs cmd in this shell, so a cd sticks
    definition: bool = False  # the `()` shell_commands.commands puts after a function's name


# Token kinds: ("word", Word), ("op", str), ("redir", str), ("nested", tokens). A "redir" is
# followed by its target word; "nested" carries the commands of a here-document's
# substitutions.
CONTROL = ("&&", "||", ";;&", ";;", ";&", "|&", ";", "|", "&", "(", ")")
REDIR = re.compile(r"(\d+)?(<<<|<<-|<<|&>>|&>|>>|>&|<&|<>|>\||>|<)")
META = set(" \t\n;&|()<>")
ANSI = {"a": "\a", "b": "\b", "e": "\x1b", "E": "\x1b", "f": "\f", "n": "\n", "r": "\r",
        "t": "\t", "v": "\v", "\\": "\\", "'": "'", '"': '"', "?": "?"}


def lex(s, i=0, stop=False):
    """(tokens, end) of `s` from `i`. With `stop`, ends after the `)` that closes a `$(` or
    `<(` opened before `i`; without it, a `)` with no `(` is returned as an operator, as is the
    `)` that ends a `case` pattern."""
    tokens, heredocs, depth, cases = [], [], 0, 0
    while i < len(s):
        c = s[i]
        if c in " \t":
            i += 1
        elif c == "\\" and s.startswith("\n", i + 1):
            i += 2
        elif c == "\n":
            tokens.append(("op", "\n"))
            i = read_heredocs(s, i + 1, heredocs, tokens)
        elif c == "#":
            while i < len(s) and s[i] != "\n":
                i += 1
        elif c in "<>" and s.startswith("(", i + 1):
            inner, i = lex(s, i + 2, stop=True)
            tokens.append(("word", Word(text=c + "(...)", dynamic=True, nested=[inner])))
        else:
            redir = REDIR.match(s, i)
            if redir:
                i = read_redirect(s, redir, tokens, heredocs)
                continue
            op = next((o for o in CONTROL if s.startswith(o, i)), None)
            if op == ")" and stop and depth == 0 and not cases:
                return tokens, i + 1
            if op:
                if not (op == ")" and depth == 0):  # a case pattern's `)` closes nothing
                    depth += {"(": 1, ")": -1}.get(op, 0)
                tokens.append(("op", op))
                i += len(op)
                continue
            word, i = read_word(s, i)
            cases += case_change(tokens, word)
            tokens.append(("word", word))
    if stop:
        raise LexError("unterminated $( or <(")
    read_heredocs(s, i, heredocs, tokens)
    return tokens, i


def case_change(tokens, word):
    """+1 for a `case` that starts a command, -1 for an `esac` that does, else 0."""
    at_start = not tokens or tokens[-1][0] == "op"
    if word.quoted or not at_start:
        return 0
    return {"case": 1, "esac": -1}.get(word.text, 0)


def read_redirect(s, redir, tokens, heredocs):
    """Add the redirection `redir` matched to `tokens`; a here-document's delimiter word is read
    now and its body at the end of the line. Returns the index after the operator."""
    op, i = redir.group(2), redir.end()
    tokens.append(("redir", op))
    if op in ("<<", "<<-"):
        delim, i = read_word(s, skip_blanks(s, i))
        heredocs.append((delim, op == "<<-"))
        tokens.append(("word", delim))
    return i


def skip_blanks(s, i):
    while i < len(s) and s[i] in " \t":
        i += 1
    return i


def read_heredocs(s, i, heredocs, tokens):
    """Read the bodies of the here-documents opened on the line just ended into their delimiter
    words. A body whose delimiter is unquoted still runs its command substitutions; their
    commands are added as a "nested" token. A missing delimiter ends the body at the end of the
    text, as bash does."""
    while heredocs:
        delim, strip_tabs = heredocs.pop(0)
        start = i
        while i < len(s):
            end = s.find("\n", i)
            end = len(s) if end < 0 else end
            line = s[i:end]
            if (line.lstrip("\t") if strip_tabs else line) == delim.text:
                body, i = s[start:i], end + 1
                break
            i = end + 1
        else:
            body = s[start:]
            i = len(s)
        delim.body = body
        if not delim.quoted:
            word = Word()
            read_double(body, 0, word, end=None)
            tokens += [("nested", n) for n in word.nested]
    return i


def read_word(s, i):
    """(Word, end) of the word starting at `s[i]`."""
    word = Word(tilde=s.startswith("~", i))
    unquoted = ""  # the word's unquoted characters, each quoted part as one \0
    while i < len(s) and s[i] not in META:
        c = s[i]
        if c not in "\\'\"$`":
            word.text += c
            unquoted += c
            i += 1
            continue
        unquoted += "\0"
        if c == "\\":
            if s.startswith("\n", i + 1):
                i += 2
                continue
            word.text += s[i + 1:i + 2]
            word.quoted = True
            i += 2
        elif c == "'":
            end = s.find("'", i + 1)
            if end < 0:
                raise LexError("unterminated '")
            word.text += s[i + 1:end]
            word.quoted = True
            i = end + 1
        elif c == '"':
            word.quoted = True
            i = read_double(s, i + 1, word, end='"')
        elif c == "$" and s.startswith("'", i + 1):
            word.quoted = True
            i = read_ansi(s, i + 2, word)
        elif c == "$" and s.startswith('"', i + 1):
            word.quoted = True
            i = read_double(s, i + 2, word, end='"')
        else:
            i = read_expansion(s, i, word)
    if re.search(r"\{[^{}]*(,|\.\.)[^{}]*\}", unquoted):
        word.split = True
    return word, i


def read_double(s, i, word, end):
    """Read the inside of a double-quoted string (or, with `end` None, a here-document body)
    into `word`; return the index after the closing quote."""
    while i < len(s):
        c = s[i]
        if end and c == end:
            return i + 1
        if c == "\\" and s[i + 1:i + 2] in ("$", "`", '"', "\\", "\n"):
            if s[i + 1] != "\n":
                word.text += s[i + 1]
            i += 2
        elif c in "$`":
            i = read_expansion(s, i, word)
        else:
            word.text += c
            i += 1
    if end:
        raise LexError('unterminated "')
    return i


def read_ansi(s, i, word):
    """Decode a `$'...'` string from `s[i]`, after its opening quote, into `word`."""
    while i < len(s):
        c = s[i]
        if c == "'":
            return i + 1
        if c != "\\" or i + 1 >= len(s):
            word.text += c
            i += 1
            continue
        e = s[i + 1]
        num = re.match(r"x([0-9a-fA-F]{1,2})|u([0-9a-fA-F]{1,4})|U([0-9a-fA-F]{1,8})|([0-7]{1,3})",
                       s[i + 1:])
        if num:
            hexa = num.group(1) or num.group(2) or num.group(3)
            code = int(hexa, 16) if hexa else int(num.group(4), 8)
            # Python has no character past U+10FFFF; dropping one only makes the guard refuse
            # more, never less, than the path bash would pass.
            word.text += chr(code) if code <= 0x10FFFF else ""
            i += 1 + num.end()
        elif e == "c" and i + 2 < len(s) and s[i + 2] != "'":
            word.text += chr(ord(s[i + 2]) & 0x1F)
            i += 3
        else:
            word.text += ANSI.get(e, "\\" + e)
            i += 2
    raise LexError("unterminated $'")


def read_expansion(s, i, word):
    """Read a `$` or backtick expansion at `s[i]` into `word`, keeping its text as written."""
    word.dynamic = True
    start = i
    if s[i] == "`":
        j, inner = i + 1, ""
        while j < len(s) and s[j] != "`":
            if s[j] == "\\" and s[j + 1:j + 2] in ("`", "$", "\\"):
                j += 1
            inner += s[j]
            j += 1
        if j >= len(s):
            raise LexError("unterminated `")
        word.nested.append(lex(inner)[0])
        i = j + 1
    elif s.startswith("$((", i) and s[matching(s, i + 3, "(", ")", depth=2) - 2:][:2] == "))":
        i = matching(s, i + 3, "(", ")", depth=2)
        read_substitutions(s[start + 3:i - 2], word)
    elif s.startswith("$(", i):
        inner, i = lex(s, i + 2, stop=True)
        word.nested.append(inner)
    elif s.startswith("${", i):
        i = matching(s, i + 2, "{", "}", depth=1)
        if s[start + 2:start + 3] in (" ", "\t", "\n", "|"):
            # bash 5.3's `${ cmd; }` and `${| cmd; }` run cmd in this shell
            word.nested.append(lex(s[start + 2:i - 1].lstrip("|"))[0])
            word.in_shell = True
        else:
            read_substitutions(s[start + 2:i - 1], word)
    else:
        name = re.match(r"[A-Za-z_][A-Za-z0-9_]*|[0-9@*#?$!-]", s[i + 1:])
        if not name:  # a lone `$` is literal
            word.dynamic = False
            word.text += "$"
            return i + 1
        i += 1 + name.end()
    word.text += s[start:i]
    return i


def read_substitutions(inner, word):
    """Add the command substitutions inside a `${...}` or `$((...))` to `word.nested`. Quotes
    inside are not followed, so a quoted `$(` counts too."""
    text = word.text
    read_double(inner, 0, word, end=None)
    word.text = text


def matching(s, i, open_, close, depth):
    """The index after the `close` that brings `depth` to zero, quotes skipped."""
    while i < len(s):
        c = s[i]
        if c == "\\":
            i += 2
            continue
        if c in "'\"":
            end = s.find(c, i + 1)
            if end < 0:
                raise LexError("unterminated quote in an expansion")
            i = end + 1
            continue
        depth += (c == open_) - (c == close)
        i += 1
        if depth == 0:
            return i
    raise LexError(f"unterminated {open_}")
