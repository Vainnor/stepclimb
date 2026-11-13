# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fails on the ways around clippy's `disallowed-methods` gate that clippy itself does not refuse.

The gate keeps `OrgScope::instance` and `OrgScope::from_credential` to their allowed call sites
(spec 02 B9, `clippy.toml`). The workspace lints deny an outer `#[allow]` and any lint attribute
without a reason; this script refuses the rest of what a lint run cannot see:

1. an inner `#![allow(...)]` anywhere (also inside `#![cfg_attr(...)]`), which clippy's
   `allow_attributes` does not catch;
2. any lint attribute naming `disallowed_methods` that is not an outer `#[expect(...)]` on a
   `fn`: an inner attribute, or an outer one on a `mod`, `impl` or other item, silences every
   call inside it;
3. any lint attribute naming a group that holds `disallowed_methods` (`clippy::all`,
   `clippy::style`) or `warnings`;
4. any `clippy.toml` or `.clippy.toml` but the root one: clippy reads the nearest file and
   ignores the root's `disallowed-methods` when a crate has its own.

Still left to review: a reasoned `#[expect]` on a wrapper `fn` that is then called freely,
`CLIPPY_CONF_DIR`, and lint flags passed on the command line.

Usage: `check-lint-escapes.py [ROOT]` checks the repository at ROOT (default: the current
directory); `check-lint-escapes.py --self-test` runs the checks against fixtures.
"""

import os
import re
import sys
import tempfile

RUST_ROOTS = ("crates",)
SKIP_DIRS = {".git", "target", "node_modules"}
LINT_ATTRS = ("allow", "expect", "warn", "deny", "forbid")
GROUPS = re.compile(r"\b(clippy\s*::\s*(all|style)|warnings)\b")
FN_ITEM = re.compile(
    r"(pub(\s*\([^)]*\))?\s+)?(const\s+)?(async\s+)?(unsafe\s+)?(extern\s+(\"[^\"]*\"\s+)?)?fn\b"
)


def rust_files(root):
    """Every `.rs` file under `crates/` and each `apps/*/src-tauri/`."""
    bases = [os.path.join(root, r) for r in RUST_ROOTS]
    apps = os.path.join(root, "apps")
    if os.path.isdir(apps):
        bases += [os.path.join(apps, a, "src-tauri") for a in sorted(os.listdir(apps))]
    for base in bases:
        for path in walk(base):
            if path.endswith(".rs"):
                yield path


def walk(base):
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            yield os.path.join(dirpath, name)


def strip_comments(src):
    """`src` with `//` and `/* */` comments blanked, keeping offsets and line numbers. Strings
    are not parsed, so text in a string can only add a finding, never hide one."""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out[i:j] = " " * (j - i)
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            out[i:j] = [c if c == "\n" else " " for c in src[i:j]]
            i = j
        else:
            i += 1
    return "".join(out)


def attributes(code):
    """Yield `(start, end, inner, body)` for each `#[...]` and `#![...]`, brackets matched."""
    for match in re.finditer(r"#\s*(!?)\s*\[", code):
        depth, j = 1, match.end()
        while j < len(code) and depth:
            depth += {"[": 1, "]": -1}.get(code[j], 0)
            j += 1
        yield match.start(), j, match.group(1) == "!", code[match.end() : j - 1]


def lint_kinds(body):
    """The lint attribute names in an attribute body, also inside `cfg_attr(...)`."""
    return set(re.findall(r"\b(" + "|".join(LINT_ATTRS) + r")\s*\(", body))


def next_item_is_fn(code, end):
    """True when the item after the attribute ending at `end` is a `fn`, skipping further
    attributes."""
    rest = code[end:]
    while True:
        rest = rest.lstrip()
        if not rest.startswith("#"):
            break
        attr = next(attributes(rest), None)
        if attr is None or attr[0] != 0:
            break
        rest = rest[attr[1] :]
    return FN_ITEM.match(rest) is not None


def check_file(path, rel):
    with open(path, encoding="utf-8") as f:
        code = strip_comments(f.read())
    problems = []
    for start, end, inner, body in attributes(code):
        kinds = lint_kinds(body)
        if not kinds:
            continue
        line = code.count("\n", 0, start) + 1
        where = f"{rel}:{line}"
        flat = " ".join(body.split())
        if inner and "allow" in kinds:
            problems.append(f"{where}: inner #![allow] (use an outer #[expect] with a reason)")
        if GROUPS.search(body):
            problems.append(f"{where}: lint attribute on a group or `warnings`: {flat}")
        if "disallowed_methods" in body:
            outer_expect = not inner and kinds == {"expect"} and body.lstrip().startswith("expect")
            if not (outer_expect and next_item_is_fn(code, end)):
                problems.append(
                    f"{where}: disallowed_methods may only be silenced by an outer "
                    f"#[expect(..., reason)] on a fn: {flat}"
                )
    return problems


def extra_clippy_configs(root):
    found = []
    for path in walk(root):
        rel = os.path.relpath(path, root)
        if os.path.basename(path) in ("clippy.toml", ".clippy.toml") and rel != "clippy.toml":
            found.append(f"{rel}: only the root clippy.toml is allowed")
    return found


def check(root):
    """The findings for the repository at `root`, and how many Rust files it read."""
    problems = extra_clippy_configs(root)
    files = 0
    for path in rust_files(root):
        files += 1
        problems += check_file(path, os.path.relpath(path, root))
    return problems, files


SPDX = "// SPDX-License-Identifier: AGPL-3.0-or-later\n"
CALL = "let _ = stepclimb_core::OrgScope::instance(id);"
FIXTURES = {
    "clean, a reasoned outer expect on a fn": (
        "crates/a/src/lib.rs",
        SPDX + "#[test]\n#[expect(\n    clippy::disallowed_methods,\n"
        '    reason = "x"\n)]\nfn p() { ' + CALL + " }\n"
        "// #![allow(clippy::disallowed_methods)] in a comment is ignored\n"
        '#[expect(dead_code, reason = "x")]\nmod m {}\n',
        0,
    ),
    "inner allow with a reason": (
        "crates/a/tests/t.rs",
        SPDX + '#![allow(clippy::disallowed_methods, reason = "x")]\n',
        2,
    ),
    "inner allow of an unrelated lint": (
        "crates/a/src/lib.rs",
        SPDX + '#![allow(dead_code, reason = "x")]\n',
        1,
    ),
    "inner allow inside cfg_attr": (
        "crates/a/src/lib.rs",
        SPDX + '#![cfg_attr(test, allow(dead_code, reason = "x"))]\n',
        1,
    ),
    "module-level inner expect": (
        "crates/a/src/lib.rs",
        SPDX + 'mod m {\n    #![expect(clippy::disallowed_methods, reason = "x")]\n}\n',
        1,
    ),
    "outer expect on a mod": (
        "crates/a/src/lib.rs",
        SPDX + '#[expect(clippy::disallowed_methods, reason = "x")]\nmod m {}\n',
        1,
    ),
    "outer allow of disallowed_methods": (
        "crates/a/src/lib.rs",
        SPDX + '#[allow(clippy::disallowed_methods, reason = "x")]\nfn p() {}\n',
        1,
    ),
    "expect on the style group": (
        "crates/a/src/lib.rs",
        SPDX + '#[expect(clippy::style, reason = "x")]\nfn p() {}\n',
        1,
    ),
    "inner allow in the Tauri app": (
        "apps/app/src-tauri/src/main.rs",
        SPDX + '#![allow(dead_code, reason = "x")]\n',
        1,
    ),
    "a crate-level clippy.toml": ("crates/a/clippy.toml", "", 1),
}


def self_test():
    failed = 0
    for name, (rel, text, want) in FIXTURES.items():
        with tempfile.TemporaryDirectory() as root:
            with open(os.path.join(root, "clippy.toml"), "w", encoding="utf-8") as f:
                f.write("")
            path = os.path.join(root, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
            got, _ = check(root)
        if len(got) != want:
            failed += 1
            print(f"lint-escapes self-test: {name}: want {want} findings, got {got}", file=sys.stderr)
    if failed:
        print(f"lint-escapes self-test: FAILED ({failed} of {len(FIXTURES)})", file=sys.stderr)
        return 1
    print(f"lint-escapes self-test: ok ({len(FIXTURES)} fixtures)")
    return 0


def main(argv):
    if argv[1:] == ["--self-test"]:
        return self_test()
    root = argv[1] if len(argv) > 1 else "."
    problems, files = check(root)
    for problem in problems:
        print(f"lint-escapes: {problem}", file=sys.stderr)
    if problems:
        print("lint-escapes: FAILED (bin/check-lint-escapes.py, spec 02 B9)", file=sys.stderr)
        return 1
    print(f"lint-escapes: ok ({files} Rust files)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
