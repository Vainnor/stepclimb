# SPDX-License-Identifier: AGPL-3.0-or-later
"""Print a Markdown fact sheet of Stepclimb's status, which `/project-status` reads and judges.

Run from any checkout: `.claude/scripts/project-status.sh`. It reads the repository it lives in,
shells out to `git` and `gh`, and changes nothing but the `origin` remote-tracking refs
(`git fetch -q origin`). The GitHub repository is the one `origin` points at. Each section reports
its own failure (no `origin`, a missing `gh`, a failed fetch) as a line in the output and the next
section still runs, so a gap in the sheet is always named. It always exits 0.

Milestones are the plan's (docs/plan/technical.md -> Milestones): M0 to M6 and 1.0, as GitHub
milestones whose titles start with the milestone id.
"""

import datetime
import json
import os
import re
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
RECENT_DAYS = 7
MILESTONES = ("M0", "M1", "M2", "M3", "M4", "M5", "1.0", "M6")
ADR_ROW = re.compile(r"^\|\s*\[(\d{4})\]\([^)]*\)\s*\|(.*)\|\s*$")
ADR_STATUSES = ("Accepted", "Open", "Proposed", "Superseded", "Rejected")
RUST_TEST = re.compile(r"^\s*#\[(?:tokio::|sqlx::)?test\b", re.M)
TS_TEST = re.compile(r"^\s*(?:it|test)\(", re.M)


def run(args, timeout=60):
    """Run a command in ROOT. Returns (ok, stdout, error text); never raises."""
    try:
        out = subprocess.run(
            args, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=timeout, check=False,
        )
    except FileNotFoundError:
        return False, "", f"{args[0]} not found on PATH"
    except subprocess.TimeoutExpired:
        return False, "", f"`{' '.join(args)}` timed out after {timeout} s"
    except OSError as err:
        return False, "", str(err)
    if out.returncode != 0:
        detail = (out.stderr.strip() or out.stdout.strip()).splitlines()
        return False, out.stdout, f"exit {out.returncode}: {detail[-1] if detail else 'no output'}"
    return True, out.stdout, ""


def read_text(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


# ---------------------------------------------------------------- pure parts (unit tested)

def repo_slug(remote_url):
    """`owner/name` of a GitHub remote URL (https or ssh), or None."""
    match = re.match(r"^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)"
                     r"([^/]+)/([^/]+?)(?:\.git)?/?$", remote_url.strip())
    return f"{match.group(1)}/{match.group(2)}" if match else None


def milestone_id(title):
    """The plan's milestone id a GitHub milestone title starts with (`M0 Foundations` -> M0)."""
    match = re.match(r"^(M\d|1\.0)\b", title.strip())
    return match.group(1) if match else None


def milestone_rows(milestones):
    """[(id, title, closed, open)] in the plan's order from GitHub's milestones JSON, then any
    milestone that is not one of the plan's, by title."""
    known, other = {}, []
    for m in milestones:
        mid = milestone_id(m["title"])
        row = (mid or "-", m["title"], m["closed_issues"], m["open_issues"])
        if mid in MILESTONES and mid not in known:
            known[mid] = row
        else:
            other.append(row)
    ordered = [known[mid] for mid in MILESTONES if mid in known]
    return ordered + sorted(other, key=lambda r: r[1])


def adr_rows(readme_text):
    """(number, title, status text) for each row of docs/adr/README.md's table."""
    rows = []
    for line in readme_text.splitlines():
        match = ADR_ROW.match(line)
        if not match:
            continue
        cells = [c.strip() for c in match.group(2).split("|")]
        rows.append((match.group(1), cells[0], " | ".join(cells[1:]).strip()))
    return rows


def adr_status(status_text):
    """The status word of an ADR row: one of ADR_STATUSES, or Other."""
    word = status_text.replace("*", "").strip().split(" ", 1)[0].rstrip(",.;:")
    return word if word in ADR_STATUSES else "Other"


def count_adr_statuses(rows):
    counts = {status: 0 for status in ADR_STATUSES + ("Other",)}
    for _, _, status_text in rows:
        counts[adr_status(status_text)] += 1
    return counts


def code_stats(root, files):
    """{unit: (language, lines, tests)} for `crates/<crate>/**.rs` and the TypeScript of `web/`,
    `apps/<app>/` and `packages/<package>/`. Returns (stats, unreadable paths). Generated files
    (`packages/api-client/src/schema.ts`) are not counted."""
    stats, unreadable = {}, []
    for path in files:
        parts = path.replace("\\", "/").split("/")
        if parts[0] == "crates" and len(parts) >= 3 and path.endswith(".rs"):
            unit, lang, pattern = parts[1], "Rust", RUST_TEST
        elif path.endswith((".ts", ".tsx")) and path != "packages/api-client/src/schema.ts":
            if parts[0] == "web":
                unit = "web"
            elif parts[0] in ("apps", "packages") and len(parts) >= 3:
                unit = f"{parts[0]}/{parts[1]}"
            else:
                continue
            lang, pattern = "TypeScript", TS_TEST
        else:
            continue
        try:
            text = read_text(os.path.join(root, path))
        except OSError:
            unreadable.append(path)
            continue
        _, lines, tests = stats.get(unit, (lang, 0, 0))
        stats[unit] = (lang, lines + len(text.splitlines()), tests + len(pattern.findall(text)))
    return stats, unreadable


def parse_worktrees(porcelain):
    """[(path, branch or '(detached)')] from `git worktree list --porcelain`."""
    trees, path, branch = [], None, None
    for line in porcelain.splitlines() + [""]:
        if line.startswith("worktree "):
            path, branch = line[len("worktree "):], "(detached)"
        elif line.startswith("branch "):
            branch = line[len("branch "):].removeprefix("refs/heads/")
        elif not line and path is not None:
            trees.append((path, branch))
            path = None
    return trees


# ---------------------------------------------------------------- sections

def github_repo():
    """(slug, None) for origin's GitHub repository, or (None, reason)."""
    ok, url, err = run(["git", "remote", "get-url", "origin"])
    if not ok:
        return None, f"no `origin` remote ({err})"
    slug = repo_slug(url)
    return (slug, None) if slug else (None, f"origin is not a GitHub URL: {url.strip()}")


def gh_json(args):
    """(ok, parsed JSON or error text) for a `gh` call."""
    ok, text, err = run(["gh"] + args)
    if not ok:
        return False, err
    try:
        return True, json.loads(text)
    except ValueError as e:
        return False, f"unparsable gh output: {e}"


def section_repo():
    out = ["## Repo", f"- Date (UTC): {datetime.datetime.now(datetime.timezone.utc):%Y-%m-%d %H:%M}"]
    slug, why = github_repo()
    out.append(f"- GitHub: {slug}" if slug else f"- GitHub: none ({why})")
    if slug:
        ok, _, err = run(["git", "fetch", "-q", "origin"])
        if not ok:
            out.append(f"- `git fetch origin` failed ({err}); origin/next below is the last fetched")
    ref = "origin/next" if slug else "next"
    ok, head, err = run(["git", "log", "-1", "--format=%h %cs %s", ref])
    out.append(f"- {ref}: {head.strip()}" if ok else f"- {ref}: unreadable ({err})")
    ok, branch, err = run(["git", "branch", "--show-current"])
    out.append(f"- Current branch: {branch.strip() or '(detached)'}" if ok
               else f"- Current branch: unreadable ({err})")
    return out


def section_milestones():
    out = ["## Milestones (GitHub)"]
    slug, why = github_repo()
    if not slug:
        return out + [f"- skipped: {why}"]
    if shutil.which("gh") is None:
        return out + ["- `gh` not found on PATH; milestones not read"]
    ok, data = gh_json(["api", f"repos/{slug}/milestones?state=all&per_page=100"])
    if not ok:
        return out + [f"- `gh api milestones` failed: {data}"]
    rows = milestone_rows(data)
    out.append("| Milestone | Title | Closed issues | Open issues |")
    out.append("| --- | --- | ---: | ---: |")
    out.extend(f"| {mid} | {title} | {closed} | {opened} |" for mid, title, closed, opened in rows)
    missing = [m for m in MILESTONES if m not in {r[0] for r in rows}]
    if missing:
        out.append(f"- plan milestones with no GitHub milestone: {', '.join(missing)}")
    ok, issues = gh_json(["issue", "list", "-R", slug, "--state", "open", "--limit", "200",
                          "--json", "number,title,milestone"])
    if not ok:
        return out + [f"- `gh issue list` failed: {issues}"]
    out.append("### Open issues")
    for issue in issues:
        milestone = (issue.get("milestone") or {}).get("title") or "(no milestone)"
        out.append(f"- #{issue['number']} {issue['title']} [{milestone}]")
    if not issues:
        out.append("- none")
    return out


def section_prs():
    out = ["## Pull requests"]
    slug, why = github_repo()
    if not slug:
        return out + [f"- skipped: {why}"]
    if shutil.which("gh") is None:
        return out + ["- `gh` not found on PATH; PRs not listed"]
    since = (datetime.datetime.now(datetime.timezone.utc)
             - datetime.timedelta(days=RECENT_DAYS)).date()
    ok, prs = gh_json(["pr", "list", "-R", slug, "--state", "open", "--limit", "100",
                       "--json", "number,title,headRefName,isDraft,updatedAt"])
    out.append("### Open")
    if not ok:
        out.append(f"- `gh pr list` failed: {prs}")
    else:
        out.extend(f"- #{p['number']} {p['title']} ({p['headRefName']})"
                   f"{' [draft]' if p['isDraft'] else ''}, updated {p['updatedAt'][:10]}" for p in prs)
        if not prs:
            out.append("- none")
    out.append(f"### Merged since {since}")
    ok, prs = gh_json(["pr", "list", "-R", slug, "--state", "merged", "--limit", "200",
                       "--search", f"merged:>={since}", "--json", "number,title,mergedAt"])
    if not ok:
        out.append(f"- `gh pr list --state merged` failed: {prs}")
    else:
        prs.sort(key=lambda p: p["mergedAt"], reverse=True)
        out.extend(f"- #{p['number']} {p['title']}, merged {p['mergedAt'][:10]}" for p in prs)
        if not prs:
            out.append("- none")
    return out


def section_adrs():
    out = ["## ADRs (docs/adr/README.md)"]
    try:
        rows = adr_rows(read_text(os.path.join(ROOT, "docs", "adr", "README.md")))
    except OSError as err:
        return out + [f"- README unreadable: {err}"]
    counts = count_adr_statuses(rows)
    out.append(f"- {len(rows)} rows: " + ", ".join(f"{k} {v}" for k, v in counts.items()))
    for number, title, status in rows:
        kind = adr_status(status)
        if kind in ("Open", "Proposed"):
            out.append(f"- {kind}: {number} {title}")
        elif kind == "Other":
            out.append(f"- Other: {number} {title}: {status[:80]}")
    return out


def section_code():
    out = ["## Code size"]
    ok, files, err = run(["git", "ls-files"])
    if not ok:
        return out + [f"- `git ls-files` failed: {err}"]
    files = files.splitlines()
    stats, unreadable = code_stats(ROOT, files)
    out.append("| Unit | Language | Lines | Tests |")
    out.append("| --- | --- | ---: | ---: |")
    for unit in sorted(stats):
        lang, lines, tests = stats[unit]
        out.append(f"| {unit} | {lang} | {lines} | {tests} |")
    if unreadable:
        out.append(f"- tracked but unreadable, not counted: {', '.join(unreadable)}")
    specs = [f for f in files if re.fullmatch(r"docs/specs/.+\.md", f) and not f.endswith("README.md")]
    adrs = [f for f in files if re.fullmatch(r"docs/adr/\d{4}-[^/]+\.md", f)
            and not f.startswith("docs/adr/0000-")]
    out.append(f"- spec files (docs/specs/**.md, READMEs excluded): {len(specs)}")
    out.append(f"- ADR files (docs/adr/NNNN-*.md, template excluded): {len(adrs)}")
    return out


def section_worktrees():
    out = ["## Worktrees"]
    ok, text, err = run(["git", "worktree", "list", "--porcelain"])
    if not ok:
        return out + [f"- `git worktree list` failed: {err}"]
    trees = parse_worktrees(text)
    agents = [t for t in trees if "/.claude/worktrees/" in t[0].replace("\\", "/")]
    out.append(f"- {len(trees)} worktrees, {len(agents)} under .claude/worktrees/")
    out.extend(f"- {os.path.basename(path)}: {branch}" for path, branch in agents)
    return out


SECTIONS = (section_repo, section_milestones, section_prs, section_adrs, section_code,
            section_worktrees)


def main():
    print("# Stepclimb project status fact sheet\n")
    for section in SECTIONS:
        try:
            lines = section()
        except Exception as err:  # noqa: BLE001 - reported, and the next section still runs
            lines = [f"## {section.__name__}", f"- section failed: {type(err).__name__}: {err}"]
        print("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
