#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Boots deploy/compose/compose.production.yaml with the tls profile and generated secrets and
# checks Caddy and the nightly dumps (docs/specs/m0/14-compose-deploy.md B6, B7, B19; T8, T9,
# T12):
#   T8   `docker compose config` accepts the tls profile, alone and with minio;
#   B6   Caddy without SC_DOMAIN exits naming it;
#   T9   through Caddy with its internal CA, /readyz answers 200 over HTTPS for the domain, with a
#        certificate that verifies against Caddy's root, and HTTP redirects to HTTPS;
#   B7   the upstream sees the request come from Caddy's fixed address on the proxy network,
#        with Host, X-Forwarded-Proto https and the client's address, and none of the
#        forwarding headers the client sent (an echo server stands in for the server);
#   B19  the backup service schedules the next dump at SC_BACKUP_AT, refuses a bad SC_BACKUP_AT,
#        deletes dumps more than SC_BACKUP_KEEP_DAYS older than the newest (or than now, when a
#        dump is dated in the future) and no others, never the dump just written, and `restore`
#        refuses a database with tables;
#   T12  a dump of a stack with data restores into a fresh stack, whose server turns healthy with
#        /readyz 200 and the data present.
# SC_OPS_CHECK_PROJECT, SC_OPS_CHECK_PORT (the server), SC_OPS_CHECK_HTTP_PORT and
# SC_OPS_CHECK_HTTPS_PORT (Caddy) and SC_OPS_CHECK_NETWORK (the proxy network's first three
# octets) override the project name, ports and subnet; the fresh stack of T12 is the project
# name with `-restore`. Like check-production-compose.sh, it never touches the
# development stack and removes its containers, volumes and secrets on exit.
set -euo pipefail

cd "$(dirname "$0")/.."

project=${SC_OPS_CHECK_PROJECT:-stepclimb-opscheck}
restored=$project-restore
# Below Linux's ephemeral range (32768 and up by default): an outgoing connection that took one
# of these as its source port would block the bind while it sits in TIME-WAIT.
port=${SC_OPS_CHECK_PORT:-28280}
http_port=${SC_OPS_CHECK_HTTP_PORT:-28281}
https_port=${SC_OPS_CHECK_HTTPS_PORT:-28443}
network=${SC_OPS_CHECK_NETWORK:-10.201.74}
domain=stepclimb.test
production=deploy/compose/compose.production.yaml
work=$(mktemp -d "${TMPDIR:-/tmp}/stepclimb-opscheck.XXXXXX")
echo_server=$project-echo
export SC_HTTP_PORT=$port SC_SECRETS_DIR="$work/secrets" SC_DOMAIN=$domain \
    SC_PUBLIC_URL=https://$domain \
    SC_CERT_ISSUER=internal SC_PROXY_ADDRESS=127.0.0.1 SC_PROXY_HTTP_PORT=$http_port \
    SC_PROXY_HTTPS_PORT=$https_port SC_PROXY_NETWORK=$network

compose() { docker compose -p "$project" -f "$production" "$@"; }
compose_restored() { docker compose -p "$restored" -f "$production" "$@"; }
fail() {
    echo "compose ops: FAILED: $*" >&2
    exit 1
}
teardown() {
    docker rm --force "$echo_server" >/dev/null 2>&1 || true
    compose --profile tls --profile minio down --volumes --remove-orphans >/dev/null 2>&1 || true
    compose_restored --profile tls --profile minio down --volumes --remove-orphans >/dev/null 2>&1 || true
}
cleanup() {
    teardown
    rm -rf "$work"
}
# A run that was killed before its cleanup leaves its stacks behind; start from nothing.
teardown
trap cleanup EXIT

# `request URL [HOST [CA]]` prints the status line's code, the Location header and the body of
# one HTTP/1.1 request to URL, sent with HOST as Host and TLS server name, verified against CA.
request() {
    python3 -I - "$@" <<'PY'
import http.client, socket, ssl, sys, urllib.parse
url = urllib.parse.urlsplit(sys.argv[1])
host = sys.argv[2] if len(sys.argv) > 2 else url.hostname
raw = socket.create_connection((url.hostname, url.port), timeout=10)
if url.scheme == "https":
    context = ssl.create_default_context(cafile=sys.argv[3])
    raw = context.wrap_socket(raw, server_hostname=host)
connection = http.client.HTTPConnection(host)
connection.sock = raw
connection.request("GET", url.path, headers={
    "Host": host,
    "X-Forwarded-For": "203.0.113.9",
    "X-Forwarded-Proto": "http",
    "X-Forwarded-Host": "attacker.example",
    "Forwarded": "for=203.0.113.9;proto=http",
    "X-Real-IP": "203.0.113.9",
    "X-Forwarded-Port": "80",
    "X-Forwarded-Prefix": "/attacker",
    "X-Forwarded-Ssl": "off",
    "X-Forwarded-Scheme": "http",
})
response = connection.getresponse()
print(response.status)
print(response.getheader("Location", ""))
print(response.read().decode())
PY
}

# T8: the tls profile parses, with and without minio.
mkdir -m 700 "$SC_SECRETS_DIR"
for profiles in "--profile tls" "--profile tls --profile minio"; do
    # shellcheck disable=SC2086 # $profiles is one or two flags and their values.
    compose $profiles config --quiet
done
echo "compose ops: config ok (tls; tls with minio)"

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

# B6: no domain, no Caddy, and the error says which variable.
if SC_DOMAIN="" compose --profile tls run --rm --no-deps caddy >"$work/no-domain.log" 2>&1; then
    fail "caddy started without SC_DOMAIN"
fi
grep -qF "caddy: SC_DOMAIN is not set" "$work/no-domain.log" \
    || fail "caddy without SC_DOMAIN did not name it: $(cat "$work/no-domain.log")"
echo "compose ops: caddy refuses to start without SC_DOMAIN, naming it"

compose --profile tls up --detach --build --wait --wait-timeout 180 \
    || { compose --profile tls logs >&2; fail "the stack with caddy did not turn healthy"; }

# T9: HTTPS through Caddy's internal CA, verified against its root.
for _ in $(seq 1 20); do
    compose cp caddy:/data/caddy/pki/authorities/local/root.crt "$work/root.crt" >/dev/null 2>&1 \
        && break
    sleep 0.5
done
[[ -s $work/root.crt ]] || fail "caddy did not create its internal root certificate"
mapfile -t answer < <(request "https://127.0.0.1:$https_port/readyz" "$domain" "$work/root.crt")
[[ ${answer[0]:-} == 200 ]] || fail "/readyz through caddy answered ${answer[0]:-nothing}"
mapfile -t answer < <(request "http://127.0.0.1:$http_port/readyz" "$domain")
[[ ${answer[0]:-} == 30[18] && ${answer[1]:-} == "https://$domain/readyz" ]] \
    || fail "HTTP on :$http_port answered ${answer[0]:-nothing} to '${answer[1]:-}', not a redirect to HTTPS"
echo "compose ops: /readyz 200 over HTTPS through caddy (internal CA), HTTP redirects to HTTPS"

# B7: swap the server for an echo server under the same name and read what the upstream sees.
compose stop stepclimb >/dev/null
cat >"$work/echo.Caddyfile" <<'CADDY'
:8080 {
	respond "from={remote_host} host={host} proto={header.X-Forwarded-Proto} forwarded-host={header.X-Forwarded-Host} forwarded=[{header.Forwarded}] real-ip=[{header.X-Real-IP}] port=[{header.X-Forwarded-Port}] prefix=[{header.X-Forwarded-Prefix}] ssl=[{header.X-Forwarded-Ssl}] scheme=[{header.X-Forwarded-Scheme}] for={header.X-Forwarded-For}"
}
CADDY
caddy_image=$(compose --profile tls config --images | grep '^caddy:')
docker run --detach --name "$echo_server" --network "${project}_proxy" --network-alias stepclimb \
    --volume "$work/echo.Caddyfile:/etc/caddy/Caddyfile:ro" "$caddy_image" >/dev/null
seen=""
for _ in $(seq 1 20); do
    mapfile -t answer < <(request "https://127.0.0.1:$https_port/" "$domain" "$work/root.crt")
    if [[ ${answer[0]:-} == 200 ]]; then seen=${answer[2]:-}; break; fi
    sleep 0.5
done
# This client is on the host, so Caddy sees it come from the proxy network's gateway, .1.
expected="from=$network.2 host=$domain proto=https forwarded-host=$domain forwarded=[] real-ip=[]"
expected+=" port=[] prefix=[] ssl=[] scheme=[] for=$network.1"
[[ $seen == "$expected" ]] || fail "the upstream saw '$seen', not '$expected'"
docker rm --force "$echo_server" >/dev/null
compose start stepclimb >/dev/null
echo "compose ops: the upstream sees Caddy's fixed address, Host, X-Forwarded-Proto https and the client's address, none of the client's forwarding headers ($seen)"

# B19: the schedule, a bad schedule, retention and the restore guard.
compose logs backup | grep -Eq "stepclimb-backup: next dump at [0-9-]+T03:00:00Z" \
    || fail "the backup service did not schedule a dump at 03:00: $(compose logs backup)"
if compose run --rm --no-deps -e SC_BACKUP_AT=25:00 backup >"$work/bad-at.log" 2>&1; then
    fail "the backup service started with SC_BACKUP_AT=25:00"
fi
grep -qF "SC_BACKUP_AT must be a UTC time as HH:MM" "$work/bad-at.log" \
    || fail "SC_BACKUP_AT=25:00 was not named: $(cat "$work/bad-at.log")"

compose exec -T postgres psql -q -U stepclimb -d stepclimb -v ON_ERROR_STOP=1 -c \
    "INSERT INTO organizations (id, slug, name) VALUES (gen_random_uuid(), 'opscheck', 'Ops check')"
# Stand-in dumps from 8 and 6 days ago and from a year ahead (a clock that jumped back), then
# two dumps by hand a second apart: with the default 7 days only the 8-day-old one goes, the
# future one is logged and ages count from now; with 1 day the 6-day-old one goes too. No dump
# just written is deleted, and neither hand-made dump pushes the other out.
compose exec -T backup sh -c 'now=$(date -u +%s); for days in 8 6 -365; do
    touch "/backups/stepclimb-$(date -u -d "@$((now - days * 86400))" +%Y%m%dT%H%M%SZ).dump"; done'
mapfile -t fakes < <(compose exec -T backup sh -c 'ls /backups')
for _ in 1 2; do
    compose exec -T backup stepclimb-backup now 2>>"$work/dumps.log"
    sleep 1.1
done
mapfile -t dumps < <(compose exec -T backup sh -c 'ls /backups')
[[ ${#dumps[@]} == 4 && ${dumps[0]} == "${fakes[1]}" && ${dumps[3]} == "${fakes[2]}" ]] \
    || fail "after two dumps with 7 days kept, /backups holds: ${dumps[*]} (stand-ins: ${fakes[*]})"
grep -qF "${fakes[2]} is dated in the future" "$work/dumps.log" \
    || fail "the future-dated dump was not logged: $(cat "$work/dumps.log")"
compose exec -T -e SC_BACKUP_KEEP_DAYS=1 backup stepclimb-backup now 2>>"$work/dumps.log"
newest=$(sed -n 's|^stepclimb-backup: wrote /backups/||p' "$work/dumps.log" | tail -n 1)
mapfile -t dumps < <(compose exec -T backup sh -c 'ls /backups')
[[ ${#dumps[@]} == 4 && ${dumps[0]} != "${fakes[1]}" && ${dumps[2]} == "$newest" ]] \
    || fail "after a dump with 1 day kept, /backups holds: ${dumps[*]} (just written: $newest)"
if compose exec -T backup stepclimb-backup restore "/backups/$newest" >"$work/guard.log" 2>&1; then
    fail "restore into a database with tables succeeded"
fi
grep -qF "the database already has" "$work/guard.log" \
    || fail "restore into a database with tables did not say why: $(cat "$work/guard.log")"
compose cp "backup:/backups/$newest" "$work/$newest" >/dev/null
echo "compose ops: backup schedules 03:00, refuses a bad time, prunes by age, restore refuses a used database"

# T12: tear the stack down and restore its dump into a fresh one.
compose --profile tls down --volumes --remove-orphans >/dev/null 2>&1
compose_restored up --detach --wait --wait-timeout 120 postgres
mkdir -m 755 "$work/restore" && cp "$work/$newest" "$work/restore/" && chmod 644 "$work/restore/$newest"
compose_restored run --rm --no-deps --volume "$work/restore:/restore:ro" backup \
    stepclimb-backup restore "/restore/$newest"
compose_restored up --detach --build --wait --wait-timeout 180 \
    || { compose_restored logs >&2; fail "the restored stack did not turn healthy"; }
mapfile -t answer < <(request "http://127.0.0.1:$port/readyz")
[[ ${answer[0]:-} == 200 ]] || fail "/readyz on the restored stack answered ${answer[0]:-nothing}"
slug=$(compose_restored exec -T postgres psql -U stepclimb -d stepclimb -tAc \
    "SELECT slug FROM organizations WHERE slug = 'opscheck'")
[[ $slug == opscheck ]] || fail "the restored database lacks the organization written before the dump"
echo "compose ops: a dump restores into a fresh stack, /readyz 200, data present"
echo "compose ops: ok"
