#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the server image (deploy/Dockerfile) for each platform named and checks it
# (docs/specs/m0/14-compose-deploy.md B21, T11):
#   - the image and the stepclimb binary in it are for that platform's architecture;
#   - its configured user resolves, in the image's own /etc/passwd, to a uid other than 0;
#   - it holds no shell (sh, bash, dash, ash, ksh, zsh or busybox) anywhere in its file system,
#     and for the machine's own platform `docker run --entrypoint sh` fails;
#   - /usr/local/bin holds the stepclimb binary and nothing else;
#   - `stepclimb check-config` runs in it and accepts a valid configuration: in a container for
#     the machine's own platform, and under QEMU user-mode emulation (qemu-aarch64 or
#     qemu-x86_64, from the Nix dev shell on Linux) for another, which Docker without a
#     registered emulator (binfmt_misc) cannot run as a container. Before an emulated run, the
#     binary's ELF interpreter must be in the image, so the run cannot use the host's loader.
#
#   bin/check-image.sh [PLATFORM...]
#
# With no platform it checks linux/amd64 and linux/arm64 on an amd64 machine, and linux/arm64
# alone on an arm64 one, saying so: deploy/Dockerfile cross-compiles from amd64 only. Images are
# tagged <SC_IMAGE>-<arch> (SC_IMAGE defaults to stepclimb:imagecheck-<pid>; `just ci` sets it
# per run) and untagged on exit; the layers stay in the build cache.
set -euo pipefail

cd "$(dirname "$0")/.."

native=$(docker version --format '{{.Server.Os}}/{{.Server.Arch}}')
platforms=("$@")
if [[ ${#platforms[@]} -eq 0 ]]; then
    case $native in
        linux/amd64) platforms=(linux/amd64 linux/arm64) ;;
        linux/arm64)
            platforms=(linux/arm64)
            echo "image check: linux/amd64 not built: deploy/Dockerfile cross-compiles from amd64 only"
            ;;
        *) platforms=("$native") ;;
    esac
fi
prefix=${SC_IMAGE:-stepclimb:imagecheck-$$}
work=$(mktemp -d "${TMPDIR:-/tmp}/stepclimb-imagecheck.XXXXXX")
tags=()

cleanup() {
    for tag in "${tags[@]}"; do docker image rm "$tag" >/dev/null 2>&1 || true; done
    rm -rf "$work"
}
trap cleanup EXIT

fail() {
    echo "image check: FAILED: $*" >&2
    exit 1
}

# A configuration check-config accepts; nothing here is a real secret.
config_env=(
    STEPCLIMB_DATABASE_URL=postgres://stepclimb@postgres:5432/stepclimb
    STEPCLIMB_ENCRYPTION_KEYS=k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=
    STEPCLIMB_PUBLIC_URL=https://lms.example.org
)

# The ELF machine of the file $1: x86_64 or aarch64.
elf_machine() {
    python3 -I - "$1" <<'PY'
import struct, sys
with open(sys.argv[1], "rb") as file:
    header = file.read(20)
if header[:4] != b"\x7fELF" or header[5] != 1:
    sys.exit("not a little-endian ELF file")
print({62: "x86_64", 183: "aarch64"}.get(struct.unpack_from("<H", header, 18)[0], "other"))
PY
}

# The ELF interpreter (PT_INTERP) of the binary $2, resolved inside the image root $1: symlinks
# are followed within $1, never into the host's file system. Prints the interpreter's path and
# fails when $1 does not hold it, so an emulated run cannot load the host's dynamic loader.
interpreter_in_root() {
    python3 -I - "$1" "$2" <<'PY'
import os, struct, sys

def fail(message):
    print(message)
    sys.exit(1)

root, binary = sys.argv[1], sys.argv[2]
with open(binary, "rb") as file:
    data = file.read()
if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
    fail("not a 64-bit little-endian ELF file")
(phoff,) = struct.unpack_from("<Q", data, 32)
phentsize, phnum = struct.unpack_from("<HH", data, 54)
interp = None
for i in range(phnum):
    kind, _, offset, _, _, size = struct.unpack_from("<IIQQQQ", data, phoff + i * phentsize)
    if kind == 3:  # PT_INTERP
        interp = data[offset:offset + size].rstrip(b"\0").decode()
if interp is None:
    fail("no ELF interpreter")

def split(path):
    return [part for part in path.split("/") if part not in ("", ".")]

parts, done, hops = split(interp), [], 0
while parts:
    part = parts.pop(0)
    if part == "..":
        if done:
            done.pop()
        continue
    done.append(part)
    host = os.path.join(root, *done)
    if os.path.islink(host):
        hops += 1
        if hops > 40:
            fail(f"{interp}: too many symbolic links")
        target = os.readlink(host)
        done.pop()
        if target.startswith("/"):
            done = []
        parts = split(target) + parts
    elif not os.path.lexists(host):
        fail(f"{interp} is not in the image")
if not os.path.isfile(os.path.join(root, *done)):
    fail(f"{interp} is not a file in the image")
print(interp)
PY
}

# The uid that the user $1 (a name, a uid, or either with :group) has in the passwd file $2.
uid_of() {
    local user=${1%%:*}
    if [[ $user =~ ^[0-9]+$ ]]; then
        echo "$user"
    else
        awk -F: -v name="$user" '$1 == name { print $3; found = 1 } END { exit !found }' "$2" \
            || fail "the image's user $user is not in its /etc/passwd"
    fi
}

check_platform() {
    local platform=$1 arch=${1#linux/} tag root machine qemu user uid container how interpreter
    case $arch in
        amd64) machine=x86_64 qemu=qemu-x86_64 ;;
        arm64) machine=aarch64 qemu=qemu-aarch64 ;;
        *) fail "no check for $platform (linux/amd64 and linux/arm64 only)" ;;
    esac
    tag="$prefix-$arch"
    tags+=("$tag")
    docker buildx build --quiet --platform "$platform" --load -t "$tag" -f deploy/Dockerfile . >/dev/null \
        || fail "the $platform build failed"

    [[ $(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$tag") == "$platform" ]] \
        || fail "$tag is not a $platform image"

    root="$work/$arch"
    mkdir -p "$root"
    container=$(docker create --platform "$platform" "$tag")
    docker export "$container" >"$work/$arch.tar"
    docker rm "$container" >/dev/null
    tar -x --no-same-owner --no-same-permissions -C "$root" -f "$work/$arch.tar"

    [[ $(elf_machine "$root/usr/local/bin/stepclimb") == "$machine" ]] \
        || fail "/usr/local/bin/stepclimb in $tag is not built for $machine"

    user=$(docker image inspect --format '{{.Config.User}}' "$tag")
    [[ -n $user ]] || fail "$tag sets no user, so it runs as root"
    uid=$(uid_of "$user" "$root/etc/passwd")
    [[ $uid != 0 ]] || fail "$tag runs as $user, uid 0"

    shells=$(tar -t -f "$work/$arch.tar" | grep -E '(^|/)(sh|bash|dash|ash|ksh|zsh|busybox)$' || true)
    [[ -z $shells ]] || fail "$tag holds a shell: $shells"
    [[ $(ls -A "$root/usr/local/bin") == stepclimb ]] \
        || fail "/usr/local/bin in $tag holds more than stepclimb: $(ls -A "$root/usr/local/bin")"

    if [[ $platform == "$native" ]]; then
        if docker run --rm --network none --entrypoint sh "$tag" -c true >"$work/sh.log" 2>&1; then
            fail "docker run --entrypoint sh succeeded in $tag"
        fi
        grep -q 'executable file not found' "$work/sh.log" \
            || fail "docker run --entrypoint sh failed for another reason: $(cat "$work/sh.log")"
        local env_flags=()
        for pair in "${config_env[@]}"; do env_flags+=(-e "$pair"); done
        docker run --rm --network none "${env_flags[@]}" "$tag" check-config >"$work/check.log" 2>&1 \
            || fail "check-config failed in $tag: $(cat "$work/check.log")"
        how="in a container"
    else
        interpreter=$(interpreter_in_root "$root" "$root/usr/local/bin/stepclimb") \
            || fail "/usr/local/bin/stepclimb in $tag cannot load from the image: $interpreter"
        qemu=$(command -v "$qemu") \
            || fail "cannot run the $arch image: $qemu is not on PATH (nix develop provides it on Linux)"
        env -i "${config_env[@]}" "$qemu" -L "$root" "$root/usr/local/bin/stepclimb" check-config \
            >"$work/check.log" 2>&1 \
            || fail "check-config failed under $qemu: $(cat "$work/check.log")"
        how="under ${qemu##*/}, loader $interpreter from the image"
    fi
    grep -qx 'stepclimb: the configuration is valid' "$work/check.log" \
        || fail "check-config did not report a valid configuration: $(cat "$work/check.log")"

    echo "image check: $platform ok (user $user, uid $uid; no shell; $machine binary; check-config ran $how)"
}

for platform in "${platforms[@]}"; do
    check_platform "$platform"
done
echo "image check: ok (${platforms[*]})"
