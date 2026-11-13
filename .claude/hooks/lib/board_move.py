# SPDX-License-Identifier: AGPL-3.0-or-later
"""Set an issue's Status on the GitHub Project linked to this repository.

`board_move.py <issue> <status>`. The repository is the one `origin` points at; the project is
the one linked to it (the first, if several are, which the output names). The status must be an
option of the project's single-select field named `Status`, matched without regard to case. It
changes one field of one item and nothing else, and exits 1 with the reason on any failure.
"""

import json
import os
import re
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib

STATUSES = ("Todo", "In progress", "In review", "Done")

FIND = """
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    projectsV2(first: 5) { nodes { id title number } }
    issue(number: $number) { id projectItems(first: 20) { nodes { id project { id } } } }
  }
}"""

FIELDS = """
query($project: ID!) {
  node(id: $project) {
    ... on ProjectV2 {
      fields(first: 50) {
        nodes { ... on ProjectV2SingleSelectField { id name options { id name } } }
      }
    }
  }
}"""

ADD = """
mutation($project: ID!, $content: ID!) {
  addProjectV2ItemById(input: {projectId: $project, contentId: $content}) { item { id } }
}"""

SET = """
mutation($project: ID!, $item: ID!, $field: ID!, $option: String!) {
  updateProjectV2ItemFieldValue(input: {projectId: $project, itemId: $item, fieldId: $field,
    value: {singleSelectOptionId: $option}}) { projectV2Item { id } }
}"""


class BoardError(Exception):
    """A reason the card was not moved."""


def repo_slug(url):
    match = re.match(r"^(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)"
                     r"([^/]+)/([^/]+?)(?:\.git)?/?$", url.strip())
    if not match:
        raise BoardError(f"origin is not a GitHub repository: {url.strip()}")
    return match.group(1), match.group(2)


def graphql(query, **variables):
    args = ["gh", "api", "graphql", "-f", f"query={query}"]
    for key, value in variables.items():
        args += ["-F" if isinstance(value, int) else "-f", f"{key}={value}"]
    out = subprocess.run(args, capture_output=True, text=True, timeout=60, check=False)
    if out.returncode != 0:
        raise BoardError(f"gh api graphql failed: {out.stderr.strip() or out.stdout.strip()}")
    data = json.loads(out.stdout)
    if data.get("errors"):
        raise BoardError("GitHub refused the query: " + "; ".join(e["message"] for e in data["errors"]))
    return data["data"]


def status_option(fields, wanted):
    """(field id, option id) of the `Status` option named `wanted`, case ignored."""
    for field in fields:
        if field and field.get("name") == "Status":
            for option in field["options"]:
                if option["name"].lower() == wanted.lower():
                    return field["id"], option["id"]
            names = ", ".join(o["name"] for o in field["options"])
            raise BoardError(f"the Status field has no option {wanted!r} (it has: {names})")
    raise BoardError("the project has no single-select field named Status")


def move(issue, status):
    origin = subprocess.run(["git", "remote", "get-url", "origin"], capture_output=True, text=True,
                            check=False)
    if origin.returncode != 0:
        raise BoardError("no `origin` remote")
    owner, name = repo_slug(origin.stdout)
    found = graphql(FIND, owner=owner, name=name, number=issue)["repository"]
    projects = found["projectsV2"]["nodes"]
    if not projects:
        raise BoardError(f"no Project is linked to {owner}/{name}")
    if found["issue"] is None:
        raise BoardError(f"{owner}/{name} has no issue #{issue}")
    project = projects[0]
    items = [i["id"] for i in found["issue"]["projectItems"]["nodes"]
             if i["project"]["id"] == project["id"]]
    item = items[0] if items else graphql(ADD, project=project["id"],
                                          content=found["issue"]["id"])["addProjectV2ItemById"]["item"]["id"]
    fields = graphql(FIELDS, project=project["id"])["node"]["fields"]["nodes"]
    field, option = status_option(fields, status)
    graphql(SET, project=project["id"], item=item, field=field, option=option)
    return f"#{issue} -> {status} on project {project['number']} ({project['title']})"


def main(argv):
    if len(argv) != 2 or not argv[0].lstrip("#").isdigit():
        print(f"usage: board-move.sh <issue number> <{'|'.join(STATUSES)}>", file=sys.stderr)
        return 2
    try:
        print(move(int(argv[0].lstrip("#")), argv[1]))
    except BoardError as error:
        print(f"board-move: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
