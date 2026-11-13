# SPDX-License-Identifier: AGPL-3.0-or-later
"""Run a Claude Code file hook against the files a Codex patch touches.

Codex edits files with `apply_patch`, reached two ways, and neither carries a `file_path`:
- the `apply_patch` tool: hook `tool_name` is `apply_patch`, the patch is `tool_input.command`
  (codex-rs `core/src/tools/handlers/apply_patch.rs`). Codex applies it in the turn
  environment's cwd, the cwd the hook receives;
- a shell command such as `apply_patch <<'EOF' ... EOF`, which `exec_command` intercepts and
  applies itself; the hook sees it as `Bash` with only the command text
  (codex-rs `core/src/tools/handlers/unified_exec/exec_command.rs`, `intercept_apply_patch`).
  Codex applies it in the call's `workdir` and any leading `cd`, and in 0.165 the hook's cwd is
  the session cwd, not the workdir. The hook cannot see where such a patch lands, so the adapter
  refuses every shell command that carries a patch; the model uses the tool instead.
The Claude hooks read `tool_input.file_path`, so for the tool this adapter pulls each path out of
the patch's file headers and runs the hook once per path with a Claude-style payload.

It fails closed: both tools refuse a call only on exit 2, so a hook that exits non-zero, an
`apply_patch` body with no file header, a malformed payload or any crash here all exit 2.

Usage: codex_adapter.py <hook script>
"""

import json
import os
import subprocess
import sys

sys.dont_write_bytecode = True  # keep __pycache__ out of .claude/hooks/lib
sys.path.insert(0, os.path.dirname(__file__))
import hookio  # noqa: E402

HOOK = "codex-adapter"
# File headers of the apply_patch grammar (codex-rs apply-patch/src/parser.rs). A Move to header
# names the destination of an Update File hunk. Codex trims each line before matching a header
# (apply-patch/src/streaming_parser.rs), so indented headers count too.
PATCH_HEADERS = ("*** Add File:", "*** Update File:", "*** Delete File:", "*** Move to:")
PATCH_BEGIN = "*** Begin Patch"
SHELL_PATCH_REFUSAL = "use the apply_patch tool; a shell patch's directory is not visible to hooks"


def patch_paths(patch):
    """Paths named by the file headers of a patch, in order, without repeats."""
    paths = []
    for line in patch.splitlines():
        stripped = line.strip()
        for header in PATCH_HEADERS:
            if stripped.startswith(header):
                path = stripped[len(header):].strip()
                if path and path not in paths:
                    paths.append(path)
    return paths


def is_shell_patch(data, command):
    """Whether a Bash call carries a patch body or a patch file header."""
    return data.get("tool_name") == "Bash" and (
        PATCH_BEGIN in command or bool(patch_paths(command))
    )


def patch_text(data):
    """The `apply_patch` tool's patch text, or None for any other call.

    Raises ValueError when the payload is malformed.
    """
    command = hookio.tool_input(data).get("command") or ""
    if not isinstance(command, str):
        raise ValueError("tool_input.command is not a string")
    if data.get("tool_name") == "apply_patch":
        return command
    return None


def claude_payload(codex_payload, path):
    """The Codex payload with its tool input replaced by a Claude Code Write of `path`."""
    payload = dict(codex_payload)
    payload["tool_name"] = "Write"
    payload["tool_input"] = {"file_path": path}
    return payload


def run_hook(script, payload):
    """Run `script` with `payload` on stdin; return (exit code, stderr)."""
    result = subprocess.run(
        ["bash", script],
        input=json.dumps(payload), capture_output=True, text=True, check=False,
    )
    return result.returncode, result.stderr


def main():
    if len(sys.argv) != 2:
        hookio.block(HOOK, "usage: codex_adapter.py <hook script>")
    script = sys.argv[1]
    if not os.path.isfile(script):
        hookio.block(HOOK, f"hook script missing: {script}")
    try:
        data = hookio.read_input()
        patch = patch_text(data)
        command = hookio.tool_input(data).get("command") or ""
    except ValueError as error:
        hookio.block(HOOK, f"could not parse the hook input ({error}), so the patch was not checked")
    if is_shell_patch(data, command):
        hookio.block(HOOK, SHELL_PATCH_REFUSAL)
    if patch is None:
        sys.exit(0)
    paths = patch_paths(patch)
    if not paths and patch.strip():
        hookio.block(HOOK, "the patch names no file, so its paths were not checked")
    for path in paths:
        code, stderr = run_hook(script, claude_payload(data, path))
        if code != 0:
            sys.stderr.write(stderr or f"{HOOK}: blocked; {os.path.basename(script)} exited {code}\n")
            sys.exit(2)
    sys.exit(0)


if __name__ == "__main__":
    hookio.run_guard(HOOK, main)
