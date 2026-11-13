#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Boots deploy/compose/compose.production.yaml with generated secrets and checks it
# (docs/specs/m0/14-compose-deploy.md T8 to T10):
#   T8   `docker compose config` accepts the development file and the production file, with and
#        without the minio profile;
#   T10  `up` without the secret files fails and names a missing file;
#   T9   `up` with generated secrets turns healthy, /readyz answers 200 on the published port,
#        and PostgreSQL publishes no port and sits only on internal networks.
# It runs as its own Compose project on its own port, so it never touches the development stack
# (project `stepclimb`, :55432, :58080), and removes its containers, volumes and secrets on exit.
# Needs the server image to build (deploy/Dockerfile); `just e2e` before it leaves it cached.
set -euo pipefail

cd "$(dirname "$0")/.."

project=${SC_PROD_CHECK_PROJECT:-stepclimb-prodcheck}
port=${SC_PROD_CHECK_PORT:-58180}
production=deploy/compose/compose.production.yaml
work=$(mktemp -d "${TMPDIR:-/tmp}/stepclimb-prodcheck.XXXXXX")
export SC_HTTP_ADDRESS=127.0.0.1 SC_HTTP_PORT=$port SC_SECRETS_DIR="$work/secrets"

compose() { docker compose -p "$project" -f "$production" "$@"; }
fail() {
    echo "production compose: FAILED: $*" >&2
    exit 1
}
cleanup() {
    compose --profile minio down --volumes --remove-orphans >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

# T8: every file and profile combination parses.
docker compose -f deploy/compose/compose.yaml --profile stack config --quiet
for profiles in "" "--profile minio"; do
    # shellcheck disable=SC2086 # $profiles is empty or one flag and its value.
    compose $profiles config --quiet
done
echo "production compose: config ok (development; production, with and without minio)"

# T10: no secret files, no start, and the error names the missing file.
mkdir -m 700 "$SC_SECRETS_DIR"
if compose up --detach --build --wait >"$work/missing.log" 2>&1; then
    fail "up succeeded without secret files"
fi
grep -qF "does not exist: $SC_SECRETS_DIR/" "$work/missing.log" \
    || fail "up without secret files did not name the missing file: $(cat "$work/missing.log")"
compose down --volumes --remove-orphans >/dev/null 2>&1
echo "production compose: refuses to start without secret files, naming the file"

# T9: generated secrets, healthy stack, /readyz through the published port.
python3 -I - "$SC_SECRETS_DIR" <<'PY'
import base64, os, secrets, sys
directory = sys.argv[1]
password = secrets.token_urlsafe(24)
files = {
    "postgres_password": password,
    "database_url": f"postgres://stepclimb:{password}@postgres:5432/stepclimb",
    "encryption_keys": "k1:" + base64.b64encode(secrets.token_bytes(32)).decode(),
}
for name, value in files.items():
    path = os.path.join(directory, name)
    with open(path, "w") as file:
        file.write(value + "\n")
    os.chmod(path, 0o644)
PY
compose up --detach --build --wait --wait-timeout 180 \
    || { compose logs >&2; fail "the stack did not turn healthy"; }

status=$(python3 -I - "http://127.0.0.1:$port/readyz" <<'PY'
import sys, urllib.request, urllib.error
try:
    print(urllib.request.urlopen(sys.argv[1], timeout=5).status)
except urllib.error.HTTPError as error:
    print(error.code)
PY
)
[[ $status == 200 ]] || fail "/readyz on :$port answered $status"

postgres=$(compose ps --quiet postgres)
published=$(docker inspect --format '{{json .HostConfig.PortBindings}}' "$postgres")
[[ $published == "{}" || $published == "null" ]] || fail "postgres publishes $published"
for network in $(docker inspect --format '{{range $name, $_ := .NetworkSettings.Networks}}{{$name}} {{end}}' "$postgres"); do
    [[ $(docker network inspect --format '{{.Internal}}' "$network") == true ]] \
        || fail "postgres is on $network, which is not internal"
done
echo "production compose: healthy, /readyz 200 on 127.0.0.1:$port, postgres on internal networks only"
echo "production compose: ok"
