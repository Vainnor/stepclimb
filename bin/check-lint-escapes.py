# SPDX-License-Identifier: AGPL-3.0-or-later
"""Fails on the ways around clippy's `disallowed-methods` gate that clippy itself does not refuse.

The gate keeps `OrgScope::instance` and `OrgScope::from_credential` to their allowed call sites
(spec 02 B9, `clippy.toml`). Threat model: this script and the workspace lints stop accidental
and casual bypass. Deliberate obfuscation is for the maintainer's line-by-line review; the forms
that only review catches are listed below, and the same list is in
`crates/stepclimb-core/src/scope.rs`.

Clippy (`allow_attributes`, `allow_attributes_without_reason`) refuses an outer `#[allow]` and any
`allow` or `expect` without a reason. This script refuses, in every `.rs` file of the repository
(outside build output and `node_modules`):

1. any inner `#![allow(...)]`, also inside `#![cfg_attr(...)]`;
2. any lint attribute naming `disallowed_methods` (or its old name `disallowed_method`) that is
   not an outer `#[expect(...)]` on a `fn`: on a file, a `mod` or an `impl` it silences every call
   inside;
3. an `allow`, `expect` or `warn` naming `clippy::all`, `clippy::style` (the group holding
   `disallowed_methods`), `warnings`, `renamed_and_removed_lints` or `unknown_lints`;
4. a `cfg(...)` or `cfg_attr(...)` predicate on `clippy`: `#[cfg(not(clippy))]` builds code that
   clippy never sees;
5. a lint attribute containing a macro variable (`$`), which hides the lint name from this text
   check, and any `#[$x]` unless the same file captures `$x` with the attribute matcher
   `#[$x:meta]`: then every value comes from a call site written as an attribute, which this
   scan reads (the doc-forwarding idiom `$(#[$attr:meta])*`).

And in configuration:

6. any `clippy.toml` or `.clippy.toml` but the root one (clippy reads the nearest file);
7. in any `Cargo.toml`, one of the names in rule 3 or `disallowed_methods` (spelled with `_` or
   `-`) set below `deny` in a `[lints]` or `[workspace.lints]` table, and a workspace member
   whose `[lints]` is not `workspace = true`;
8. in any `.cargo/config` or `.cargo/config.toml`, an `include` of another file, `rustflags` (in
   `[build]` or a `[target]`), `rustc-wrapper`, `rustc-workspace-wrapper`, an `alias.clippy`, and
   `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CLIPPY_ARGS` or `CLIPPY_CONF_DIR` in `[env]`.

`just lint` runs clippy with `--all-features`, so feature-gated code is linted too.

Comments and string, raw string, byte string and char literals are blanked before the attribute
scan, so neither hides an attribute nor fakes one.

Review-only (no mechanical check):

- a reasoned outer `#[expect]` on a wrapper `fn` that calls a constructor and is then called
  freely;
- `#[path = "..."]` or `include!` of a file outside the scanned tree, and code a build script
  generates;
- a lint attribute assembled by one macro and forwarded through another's `#[$x:meta]`, or
  produced by a procedural macro;
- lint flags or `CLIPPY_CONF_DIR` set in the shell environment, on the command line or in a
  Cargo configuration outside the repository, and environment the repository sets without this
  script reading it: the `flake.nix` dev shell and `export`s in `justfile` and `just/*.just`;
- the `build.rustc` and `[host]` keys of a `.cargo/config`;
- code gated on a platform (`target_os` and the like): the gate runs on Linux only (ADR 0021).

Usage: `check-lint-escapes.py [ROOT]` checks the repository at ROOT (default: the current
directory); `check-lint-escapes.py --self-test` runs the checks against the fixtures below.
"""

import os
import re
import sys
import tempfile
import tomllib

LINT_ATTRS = ("allow", "expect", "warn", "deny", "forbid")
SILENCING = {"allow", "expect", "warn"}
DISALLOWED = re.compile(r"\bdisallowed_methods?\b")
WIDE = re.compile(
    r"\b(clippy\s*::\s*(all|style)|warnings|renamed_and_removed_lints|unknown_lints)\b"
)
# Cargo accepts lint names in `[lints]` with `-` or `_`; compared after `-` becomes `_`.
CARGO_GUARDED = {
    "disallowed_methods",
    "disallowed_method",
    "all",
    "style",
    "warnings",
    "renamed_and_removed_lints",
    "unknown_lints",
}
ENV_REFUSED = {"RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CLIPPY_ARGS", "CLIPPY_CONF_DIR"}
FN_ITEM = re.compile(
    r"(pub(\s*\([^)]*\))?\s+)?(const\s+)?(async\s+)?(unsafe\s+)?(extern\s+(\"[^\"]*\"\s+)?)?fn\b"
)
CFG = re.compile(r"\bcfg(_attr)?\s*\(")
CLIPPY_CFG = re.compile(r"\bclippy\b(?!\s*::)")
STRING_START = re.compile(r'(?:[bc]?r(#*)")|(?:[bc]?")')


# --- Files -------------------------------------------------------------------------------------


def walk(root):
    """Every file under `root`, skipping `.git`, `node_modules`, the top-level `target/`, each
    app's `src-tauri/target/`, and `.claude/worktrees/` (other checkouts, checked in their own
    runs)."""
    skipped = {
        os.path.join(root, "target"),
        os.path.join(root, ".claude", "worktrees"),
    }
    apps = os.path.join(root, "apps")
    if os.path.isdir(apps):
        skipped |= {os.path.join(apps, a, "src-tauri", "target") for a in os.listdir(apps)}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(
            d
            for d in dirnames
            if d not in (".git", "node_modules") and os.path.join(dirpath, d) not in skipped
        )
        for name in sorted(filenames):
            yield os.path.join(dirpath, name)


# --- Rust source -------------------------------------------------------------------------------


def is_ident(ch):
    return ch.isalnum() or ch == "_"


def literal_end(src, i):
    """The end of the comment or literal starting at `i`, or None if none starts there."""
    if src.startswith("//", i):
        j = src.find("\n", i)
        return len(src) if j < 0 else j
    if src.startswith("/*", i):
        return block_comment_end(src, i)
    if src[i] == "'":
        return char_end(src, i)
    if i > 0 and is_ident(src[i - 1]):
        return None
    match = STRING_START.match(src, i)
    if not match:
        return None
    if match.group(1) is not None:  # raw string: no escapes, ends at `"` and as many `#`
        j = src.find('"' + match.group(1), match.end())
        return len(src) if j < 0 else j + 1 + len(match.group(1))
    return quoted_end(src, match.end(), '"')


def block_comment_end(src, i):
    depth, j = 0, i
    while j < len(src):
        if src.startswith("/*", j):
            depth, j = depth + 1, j + 2
        elif src.startswith("*/", j):
            depth, j = depth - 1, j + 2
            if depth == 0:
                return j
        else:
            j += 1
    return len(src)


def char_end(src, i):
    """A char or byte literal (`'x'`, `'\\n'`), or None for a lifetime or label (`'a`)."""
    if src.startswith("\\", i + 1):
        return quoted_end(src, i + 1, "'")
    if i + 2 < len(src) and src[i + 2] == "'":
        return i + 3
    return None


def quoted_end(src, j, quote):
    while j < len(src):
        if src[j] == "\\":
            j += 2
        elif src[j] == quote:
            return j + 1
        else:
            j += 1
    return len(src)


def strip_code(src):
    """`src` with comments and literals blanked, keeping offsets and line numbers."""
    out, i = list(src), 0
    while i < len(src):
        j = literal_end(src, i)
        if j is None:
            i += 1
            continue
        out[i:j] = [c if c == "\n" else " " for c in src[i:j]]
        i = j
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
    while (rest := rest.lstrip()).startswith("#"):
        attr = next(attributes(rest), None)
        if attr is None or attr[0] != 0:
            break
        rest = rest[attr[1] :]
    return FN_ITEM.match(rest) is not None


META_MATCHER = re.compile(r"^\s*\$\s*(\w+)\s*:\s*meta\s*$")
META_USE = re.compile(r"^\s*\$\s*(\w+)\s*$")


def meta_matchers(code):
    """The macro variables a file captures with `#[$x:meta]`."""
    return {m.group(1) for *_, body in attributes(code) if (m := META_MATCHER.match(body))}


def macro_attribute_problems(body, matchers):
    if META_MATCHER.match(body):
        return []
    use = META_USE.match(body)
    if use and use.group(1) in matchers:
        return []
    return ["attribute from a macro variable not captured as #[$x:meta] hides what it sets"]


def attribute_problems(code, end, inner, body, matchers):
    kinds = lint_kinds(body)
    if body.lstrip().startswith("$"):
        return macro_attribute_problems(body, matchers)
    problems = []
    if CFG.search(body) and CLIPPY_CFG.search(body):
        problems.append("a cfg on `clippy` hides code from the lint run")
    if not kinds:
        return problems
    if "$" in body:
        problems.append("lint attribute with a macro variable hides the lint name")
    if inner and "allow" in kinds:
        problems.append("inner #![allow] (use an outer #[expect] with a reason)")
    if kinds & SILENCING and WIDE.search(body):
        problems.append("lint attribute on a group, `warnings` or the lint-name lints")
    if DISALLOWED.search(body):
        outer_expect = not inner and kinds == {"expect"} and body.lstrip().startswith("expect")
        if not (outer_expect and next_item_is_fn(code, end)):
            problems.append(
                "disallowed_methods may only be silenced by an outer #[expect(..., reason)] on a fn"
            )
    return problems


def rust_problems(path, rel):
    with open(path, encoding="utf-8") as f:
        code = strip_code(f.read())
    problems, matchers = [], meta_matchers(code)
    for start, end, inner, body in attributes(code):
        line = code.count("\n", 0, start) + 1
        flat = " ".join(body.split())
        for problem in attribute_problems(code, end, inner, body, matchers):
            problems.append(f"{rel}:{line}: {problem}: #[{flat}]")
    return problems


# --- Configuration -----------------------------------------------------------------------------


def read_toml(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def lint_level_problems(table, where):
    problems = []
    for tool, entries in (table or {}).items():
        if tool == "workspace" or not isinstance(entries, dict):
            continue
        for name, setting in entries.items():
            level = setting.get("level") if isinstance(setting, dict) else setting
            if name.replace("-", "_") in CARGO_GUARDED and level not in ("deny", "forbid"):
                problems.append(f"{where}: [{tool}] {name} = {level!r} is below deny")
    return problems


def manifest_problems(path, rel):
    manifest = read_toml(path)
    problems = lint_level_problems(manifest.get("lints"), f"{rel} [lints]")
    workspace = manifest.get("workspace", {})
    problems += lint_level_problems(workspace.get("lints"), f"{rel} [workspace.lints]")
    if "package" in manifest and "workspace" not in manifest:
        if manifest.get("lints") != {"workspace": True}:
            problems.append(f"{rel}: a workspace member's [lints] must be `workspace = true`")
    return problems


def cargo_config_problems(path, rel):
    config = read_toml(path)
    found = ["include"] if "include" in config else []
    build = config.get("build", {})
    found += [f"build.{k}" for k in ("rustflags", "rustc-wrapper", "rustc-workspace-wrapper") if k in build]
    found += [f"target.{t}.rustflags" for t, v in config.get("target", {}).items() if "rustflags" in v]
    found += ["alias.clippy"] if "clippy" in config.get("alias", {}) else []
    found += [f"env.{k}" for k in config.get("env", {}) if k in ENV_REFUSED]
    return [f"{rel}: {key} can change what clippy checks" for key in found]


def config_problems(path, rel):
    name = os.path.basename(path)
    parent = os.path.basename(os.path.dirname(path))
    if name in ("clippy.toml", ".clippy.toml") and rel != "clippy.toml":
        return [f"{rel}: only the root clippy.toml is allowed"]
    if name == "Cargo.toml":
        return manifest_problems(path, rel)
    if parent == ".cargo" and name in ("config", "config.toml"):
        return cargo_config_problems(path, rel)
    return []


def check(root):
    """The findings for the repository at `root`, and how many Rust files it read."""
    problems, files = [], 0
    for path in walk(root):
        rel = os.path.relpath(path, root)
        if path.endswith(".rs"):
            files += 1
            problems += rust_problems(path, rel)
        else:
            problems += config_problems(path, rel)
    return problems, files


# --- Self-test ---------------------------------------------------------------------------------

SPDX = "// SPDX-License-Identifier: AGPL-3.0-or-later\n"
LIB = "crates/a/src/lib.rs"
EXPECT_DM = '#[expect(clippy::disallowed_methods, reason = "x")]\n'
MEMBER = '[package]\nname = "a"\n\n[lints]\nworkspace = true\n'
CLEAN_RS = (
    SPDX
    + "#[test]\n#[expect(\n    clippy::disallowed_methods,\n"
    + '    reason = "x"\n)]\n#[cfg(test)]\npub(crate) async fn p() {}\n'
    + "// #![allow(clippy::disallowed_methods)] in a comment\n"
    + "/* outer /* nested */ #![allow(dead_code)] still a comment */\n"
    + 'const S: &str = "#![allow(dead_code)] in a string";\n'
    + 'const R: &str = r#"a " quote and #![allow(dead_code)]"#;\n'
    + "fn l<'a>(x: &'a str) -> char { let _ = '\"'; let _ = b'\\''; 'q' }\n"
    + '#[expect(dead_code, reason = "x")]\nmod m {}\n'
)
FIXTURES = {
    "clean": ({LIB: CLEAN_RS, "crates/a/Cargo.toml": MEMBER, ".cargo/config.toml": '[env]\nSQLX_OFFLINE = "true"\n'}, 0),
    "inner allow with a reason": ({LIB: SPDX + '#![allow(clippy::disallowed_methods, reason = "x")]\n'}, 2),
    "inner allow of another lint": ({LIB: SPDX + '#![allow(dead_code, reason = "x")]\n'}, 1),
    "inner allow inside cfg_attr": ({LIB: SPDX + '#![cfg_attr(test, allow(dead_code, reason = "x"))]\n'}, 1),
    "module-level inner expect": ({LIB: SPDX + 'mod m {\n    #![expect(clippy::disallowed_methods, reason = "x")]\n}\n'}, 1),
    "outer expect on a mod": ({LIB: SPDX + EXPECT_DM + "mod m {}\n"}, 1),
    "outer expect on an impl": ({LIB: SPDX + EXPECT_DM + "impl S {}\n"}, 1),
    "outer allow of disallowed_methods": ({LIB: SPDX + '#[allow(clippy::disallowed_methods, reason = "x")]\nfn p() {}\n'}, 1),
    "the old lint name": ({LIB: SPDX + '#[expect(clippy::disallowed_method, reason = "x")]\nmod m {}\n'}, 1),
    "expect on the style group": ({LIB: SPDX + '#[expect(clippy::style, reason = "x")]\nfn p() {}\n'}, 1),
    "expect on renamed_and_removed_lints": ({LIB: SPDX + '#[expect(renamed_and_removed_lints, reason = "x")]\nfn p() {}\n'}, 1),
    "allow of unknown_lints": ({LIB: SPDX + '#![allow(unknown_lints, reason = "x")]\n'}, 2),
    "a string holding /* hides nothing": ({LIB: SPDX + 'const A: &str = "/*";\n' + EXPECT_DM + 'mod m {}\nconst B: &str = "*/";\n'}, 1),
    "a raw string holding a quote hides nothing": ({LIB: SPDX + 'const A: &str = r#"x"/*"#;\n' + EXPECT_DM + 'mod m {}\nconst B: &str = "*/";\n'}, 1),
    "a char quote hides nothing": ({LIB: SPDX + "const A: char = '\"';\n" + EXPECT_DM + "mod m {}\nconst B: char = '\"';\n"}, 1),
    "a lint name from a macro variable": ({LIB: SPDX + 'macro_rules! m { ($l:path) => { #[expect($l, reason = "x")] fn p() {} } }\n'}, 1),
    "an attribute from a macro variable": ({LIB: SPDX + "macro_rules! m { ($a:meta) => { #[$a] mod q {} } }\n"}, 1),
    "doc forwarding through #[$x:meta] is allowed": ({LIB: SPDX + "macro_rules! m { ($(#[$a:meta])* $n:ident) => { $(#[$a])* struct $n; } }\n"}, 0),
    "a call site's attribute is still read": ({LIB: SPDX + "macro_rules! m { ($(#[$a:meta])* $n:ident) => { $(#[$a])* struct $n; } }\nm! { " + EXPECT_DM + "S }\n"}, 1),
    "inner allow outside crates/": ({"tools/x/src/main.rs": SPDX + '#![allow(dead_code, reason = "x")]\n'}, 1),
    "inner allow in the Tauri app": ({"apps/app/src-tauri/src/main.rs": SPDX + '#![allow(dead_code, reason = "x")]\n'}, 1),
    "build output and node_modules are skipped": ({"target/x.rs": "#![allow(dead_code)]\n", "web/node_modules/p/x.rs": "#![allow(dead_code)]\n", "apps/app/src-tauri/target/x.rs": "#![allow(dead_code)]\n"}, 0),
    "a crate-level clippy.toml": ({"crates/a/clippy.toml": ""}, 1),
    "a member without workspace lints": ({"crates/a/Cargo.toml": '[package]\nname = "a"\n\n[lints.clippy]\ntodo = "deny"\n'}, 1),
    "a member with no [lints]": ({"crates/a/Cargo.toml": '[package]\nname = "a"\n'}, 1),
    "workspace lints allow disallowed_methods": ({"Cargo.toml": '[workspace]\n\n[workspace.lints.clippy]\ndisallowed_methods = "allow"\n'}, 1),
    "workspace lints lower the style group": ({"Cargo.toml": '[workspace]\n\n[workspace.lints.clippy]\nstyle = { level = "allow", priority = -1 }\n'}, 1),
    "a standalone workspace allows warnings": ({"apps/app/src-tauri/Cargo.toml": '[package]\nname = "app"\n\n[workspace]\n\n[lints.rust]\nwarnings = "allow"\n'}, 1),
    "build rustflags": ({".cargo/config.toml": '[build]\nrustflags = ["-A", "clippy::all"]\n'}, 1),
    "target rustflags": ({".cargo/config.toml": "[target.x86_64-unknown-linux-gnu]\nrustflags = []\n"}, 1),
    "env CLIPPY_CONF_DIR and RUSTFLAGS": ({"crates/a/.cargo/config": '[env]\nCLIPPY_CONF_DIR = "x"\nRUSTFLAGS = "x"\n'}, 2),
    "an alias for clippy": ({".cargo/config.toml": '[alias]\nclippy = "check"\n'}, 1),
    "a config include": ({".cargo/config.toml": 'include = "other.toml"\n'}, 1),
    "build rustc-wrapper and rustc-workspace-wrapper": ({".cargo/config.toml": '[build]\nrustc-wrapper = "x"\nrustc-workspace-wrapper = "x"\n'}, 2),
    "env CARGO_ENCODED_RUSTFLAGS and CLIPPY_ARGS": ({".cargo/config.toml": '[env]\nCARGO_ENCODED_RUSTFLAGS = "x"\nCLIPPY_ARGS = "x"\n'}, 2),
    "lint names with dashes in Cargo.toml": ({"Cargo.toml": '[workspace]\n\n[workspace.lints.clippy]\n"disallowed-methods" = "allow"\n\n[workspace.lints.rust]\nunknown-lints = "allow"\nrenamed-and-removed-lints = "warn"\n'}, 3),
    "a cfg hiding code from clippy": ({LIB: SPDX + "#[cfg(not(clippy))]\nfn p() {}\n"}, 1),
    "a cfg_attr on clippy": ({LIB: SPDX + '#![cfg_attr(clippy, expect(dead_code, reason = "x"))]\n'}, 1),
    "a cfg_attr with a clippy lint path is fine": ({LIB: SPDX + '#[cfg_attr(test, expect(clippy::unwrap_used, reason = "x"))]\nfn p() {}\n'}, 0),
    "expect on clippy::all": ({LIB: SPDX + '#[expect(clippy::all, reason = "x")]\nfn p() {}\n'}, 1),
    "expect on warnings": ({LIB: SPDX + '#[expect(warnings, reason = "x")]\nfn p() {}\n'}, 1),
    "warn on the style group": ({LIB: SPDX + "#[warn(clippy::style)]\nfn p() {}\n"}, 1),
    "a .clippy.toml": ({"crates/a/.clippy.toml": ""}, 1),
}


def run_fixture(files):
    with tempfile.TemporaryDirectory() as root:
        for rel, text in {"clippy.toml": "", **files}.items():
            path = os.path.join(root, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
        return check(root)[0]


def self_test():
    failed = 0
    for name, (files, want) in FIXTURES.items():
        got = run_fixture(files)
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
