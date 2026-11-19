#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds the server image (deploy/Dockerfile) for each platform named and checks it
# (docs/specs/m0/14-compose-deploy.md B21, T11):
#   - the image and the stepclimb binary in it are for that platform's architecture;
#   - its configured user resolves, in the image's own /etc/passwd, to a uid other than 0;
#   - it holds no shell (sh, bash, dash, ash, ksh, zsh or busybox) anywhere in its file system,
#     and for the machine's own platform `docker run --entrypoint sh` fails;
#   - /usr/local/bin holds stepclimb, ffmpeg, ffprobe and prlimit and nothing else, each built for
#     that architecture, and stepclimb carries cargo-auditable's `.dep-v0` section, the crate list
#     the release SBOM reads (ADR 0031);
#   - ffmpeg, ffprobe and prlimit report their versions, ffmpeg transcodes a 1 s `testsrc` clip to
#     one 360p HLS rendition, and /LICENSES holds their licenses and source offers
#     (docs/specs/m1/08-uploaded-video.md T17, ADR 0035). In the native image the transcode runs
#     under prlimit as the transcoder starts it, and a run under a 16 MiB address space must fail,
#     so the limits hold in the container. Under QEMU user-mode emulation ffmpeg runs directly:
#     an emulated prlimit cannot start a second emulated program without a registered emulator.
#     The transcode's output is checked with the host's ffprobe (the Nix dev shell's);
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

# Whether the 64-bit little-endian ELF file $1 has a section named $2.
has_section() {
    python3 -I - "$1" "$2" <<'PY'
import struct, sys
with open(sys.argv[1], "rb") as file:
    data = file.read()
if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
    sys.exit("not a 64-bit little-endian ELF file")
(shoff,) = struct.unpack_from("<Q", data, 40)
shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 58)
names_offset = struct.unpack_from("<Q", data, shoff + shstrndx * shentsize + 24)[0]
for i in range(shnum):
    (name,) = struct.unpack_from("<I", data, shoff + i * shentsize)
    end = data.index(b"\0", names_offset + name)
    if data[names_offset + name:end].decode() == sys.argv[2]:
        sys.exit(0)
sys.exit(1)
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

# Runs the command after $1 and fails unless it succeeds and its first output line starts with $1.
reports() {
    local prefix=$1 output
    shift
    output=$("$@" 2>&1) || return 1
    [[ ${output%%$'\n'*} == "$prefix"* ]]
}

# Checks the HLS output in the directory $1: a finished playlist naming seg_0.ts, and that segment
# an H.264 stream of 640x360. $2 says where it ran.
check_hls() {
    local dir=$1 where=$2 stream
    grep -qx '#EXT-X-ENDLIST' "$dir/index.m3u8" 2>/dev/null \
        || fail "the HLS transcode $where wrote no finished playlist"
    grep -qx 'seg_0.ts' "$dir/index.m3u8" || fail "the HLS playlist $where names no seg_0.ts"
    command -v ffprobe >/dev/null || fail "ffprobe is not on PATH to check the transcode (nix develop provides it)"
    stream=$(ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,width,height \
        -of csv=p=0 "$dir/seg_0.ts") || fail "the HLS segment $where does not read"
    # A transport stream lists the stream under its program too: the first line is enough.
    stream=${stream%%$'\n'*}
    [[ $stream == h264,640,360 ]] || fail "the HLS segment $where is $stream, not h264,640,360"
}

# ffmpeg's arguments for the 1 s 360p HLS transcode into the directory $1.
hls_args() {
    echo -nostdin -hide_banner -loglevel error -xerror -threads 2 \
        -f lavfi -i testsrc=size=640x360:rate=25:duration=1 \
        -c:v libx264 -preset veryfast -pix_fmt yuv420p \
        -f hls -hls_time 1 -hls_playlist_type vod -hls_segment_filename "$1/seg_%d.ts" "$1/index.m3u8"
}

# The limits the transcoder gives an ffmpeg run (stepclimb_media::plan, spec 08 D42), with an
# address space of $1 bytes.
prlimit_args() {
    echo "--as=$1" --fsize=104857600 --nofile=256 --cpu=60:70 --
}

# ffmpeg, ffprobe and prlimit in the native image $1: versions, the transcode under prlimit, and
# a run prlimit must stop.
check_tools_native() {
    local tag=$1 c out
    local run=(docker run --rm --network none --entrypoint)
    reports 'ffmpeg version ' "${run[@]}" /usr/local/bin/ffmpeg "$tag" -hide_banner -version \
        || fail "ffmpeg -version failed in $tag"
    reports 'ffprobe version ' "${run[@]}" /usr/local/bin/ffprobe "$tag" -hide_banner -version \
        || fail "ffprobe -version failed in $tag"
    reports 'prlimit from util-linux ' "${run[@]}" /usr/local/bin/prlimit "$tag" --version \
        || fail "prlimit --version failed in $tag"
    # shellcheck disable=SC2046 # the argument lists hold no spaces
    c=$(docker create --network none --entrypoint /usr/local/bin/prlimit "$tag" \
        $(prlimit_args 5368709120) /usr/local/bin/ffmpeg $(hls_args /tmp))
    if ! docker start -a "$c" >"$work/hls.log" 2>&1; then
        docker rm "$c" >/dev/null
        fail "the HLS transcode under prlimit failed in $tag: $(cat "$work/hls.log")"
    fi
    out="$work/hls-native"
    mkdir -p "$out"
    if ! { docker cp "$c:/tmp/index.m3u8" "$out/" && docker cp "$c:/tmp/seg_0.ts" "$out/"; } >/dev/null; then
        docker rm "$c" >/dev/null
        fail "the HLS transcode in $tag left no output"
    fi
    docker rm "$c" >/dev/null
    check_hls "$out" "in $tag"
    # shellcheck disable=SC2046
    if docker run --rm --network none --entrypoint /usr/local/bin/prlimit "$tag" \
        $(prlimit_args 16777216) /usr/local/bin/ffmpeg -hide_banner -version >/dev/null 2>&1; then
        fail "ffmpeg ran under a 16 MiB address space in $tag: prlimit set no limit"
    fi
}

# ffmpeg, ffprobe and prlimit of the image root $1 under the emulator $2.
check_tools_emulated() {
    local root=$1 qemu=$2 out="$work/hls-emulated"
    reports 'ffmpeg version ' env -i "$qemu" "$root/usr/local/bin/ffmpeg" -hide_banner -version \
        || fail "ffmpeg -version failed under ${qemu##*/}"
    reports 'ffprobe version ' env -i "$qemu" "$root/usr/local/bin/ffprobe" -hide_banner -version \
        || fail "ffprobe -version failed under ${qemu##*/}"
    interpreter_in_root "$root" "$root/usr/local/bin/prlimit" >/dev/null \
        || fail "/usr/local/bin/prlimit cannot load from the image"
    reports 'prlimit from util-linux ' env -i "$qemu" -L "$root" "$root/usr/local/bin/prlimit" --version \
        || fail "prlimit --version failed under ${qemu##*/}"
    mkdir -p "$out"
    # shellcheck disable=SC2046
    env -i "$qemu" "$root/usr/local/bin/ffmpeg" $(hls_args "$out") >"$work/hls.log" 2>&1 \
        || fail "the HLS transcode failed under ${qemu##*/}: $(cat "$work/hls.log")"
    check_hls "$out" "under ${qemu##*/}"
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
    local platform=$1 arch=${1#linux/} tag root machine qemu user uid container how interpreter program file
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

    for program in stepclimb ffmpeg ffprobe prlimit; do
        [[ $(elf_machine "$root/usr/local/bin/$program") == "$machine" ]] \
            || fail "/usr/local/bin/$program in $tag is not built for $machine"
    done
    for file in README common-licenses/GPL-3 ffmpeg/SOURCE ffmpeg/versions.json util-linux/SOURCE \
        util-linux/copyright; do
        [[ -s $root/LICENSES/$file ]] || fail "$tag has no /LICENSES/$file"
    done

    has_section "$root/usr/local/bin/stepclimb" .dep-v0 \
        || fail "/usr/local/bin/stepclimb in $tag has no .dep-v0 section (cargo auditable build in deploy/Dockerfile)"

    user=$(docker image inspect --format '{{.Config.User}}' "$tag")
    [[ -n $user ]] || fail "$tag sets no user, so it runs as root"
    uid=$(uid_of "$user" "$root/etc/passwd")
    [[ $uid != 0 ]] || fail "$tag runs as $user, uid 0"

    shells=$(tar -t -f "$work/$arch.tar" | grep -E '(^|/)(sh|bash|dash|ash|ksh|zsh|busybox)$' || true)
    [[ -z $shells ]] || fail "$tag holds a shell: $shells"
    [[ $(LC_ALL=C ls -A "$root/usr/local/bin" | tr '\n' ' ') == "ffmpeg ffprobe prlimit stepclimb " ]] \
        || fail "/usr/local/bin in $tag holds other than ffmpeg, ffprobe, prlimit and stepclimb: $(ls -A "$root/usr/local/bin")"

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
        check_tools_native "$tag"
        how="in a container"
    else
        interpreter=$(interpreter_in_root "$root" "$root/usr/local/bin/stepclimb") \
            || fail "/usr/local/bin/stepclimb in $tag cannot load from the image: $interpreter"
        qemu=$(command -v "$qemu") \
            || fail "cannot run the $arch image: $qemu is not on PATH (nix develop provides it on Linux)"
        env -i "${config_env[@]}" "$qemu" -L "$root" "$root/usr/local/bin/stepclimb" check-config \
            >"$work/check.log" 2>&1 \
            || fail "check-config failed under $qemu: $(cat "$work/check.log")"
        check_tools_emulated "$root" "$qemu"
        how="under ${qemu##*/}, loader $interpreter from the image"
    fi
    grep -qx 'stepclimb: the configuration is valid' "$work/check.log" \
        || fail "check-config did not report a valid configuration: $(cat "$work/check.log")"

    echo "image check: $platform ok (user $user, uid $uid; no shell; $machine binaries, stepclimb with .dep-v0; check-config, ffmpeg, ffprobe, prlimit and a 360p HLS transcode ran $how)"
}

for platform in "${platforms[@]}"; do
    check_platform "$platform"
done
echo "image check: ok (${platforms[*]})"
