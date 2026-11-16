# Self-hosting Stepclimb with Docker Compose

This guide runs Stepclimb on one host with Docker Compose, from `compose.production.yaml` in this
directory. It is the self-hosting guide until the documentation site exists (M4). It describes
the M0 release: what is built today, and what is not.

`compose.yaml` beside it is the development stack and is **not for production**: its database
password and its encryption key (`development-encryption-keys`) are published in this
repository, and the server warns at startup when it finds that key in its key ring.

## What M0 has and does not have

The production example runs the server and PostgreSQL 16, with a nightly database dump, and
optionally Caddy for HTTPS and MinIO for object storage. The server applies its database
migrations at startup and answers two health probes.

Not available in M0:

- **Signing in.** `stepclimb admin create` creates the organization and its first admin and
  prints the admin's one-time enrollment link, but the web page that link opens, and the
  passkey and password set-up behind it, come with later M0 work, as do OIDC and OAuth sign-in.
- **Published images.** No image is pushed to a registry yet; you build it from this checkout.
- **Point-in-time recovery**, a Helm chart, metrics and a bare-metal binary: later milestones.

## Requirements

- Docker Engine with the Compose plugin. The example is tested with Docker Engine 29.9.2 and
  Compose 5.7.0 on x86_64 Linux, rootless.
- A checkout of this repository: the server image is built from it.
- `openssl`, for the commands below that make secrets.
- For HTTPS with a public certificate: a domain whose DNS points at the host, with ports 80 and
  443 reachable from the internet.
- About 1 to 2 vCPU and 2 GB of memory for one small organization: the plan's estimate for its
  first single-organization deployment. A larger organization may need more, and no load test
  has measured either yet.

The image builds for linux/amd64 and linux/arm64. An amd64 host builds both; an arm64 host builds
arm64 only.

## Quick start

Every command runs from the repository root.

1. Create the secret files and point `SC_SECRETS_DIR` at them (next section).
2. Say where users will open Stepclimb. Compose reads `deploy/compose/.env`, which git ignores:

   ```sh
   echo 'SC_PUBLIC_URL=https://lms.example.org' >> deploy/compose/.env
   ```

3. Build the image and start the stack:

   ```sh
   docker compose -f deploy/compose/compose.production.yaml up -d --build --wait
   ```

   `--wait` returns once PostgreSQL and the server report healthy.

4. Check it: `curl http://127.0.0.1:8080/readyz` answers `{"status":"ok"}`.

5. Create the organization and its first admin. The slug is fixed once created:

   ```sh
   docker compose -f deploy/compose/compose.production.yaml run --rm stepclimb \
     admin create --org-slug example --org-name "Example Training" \
     --email admin@example.org --name "Ada Admin"
   ```

   It exits 0 and prints the new organization's and user's ids, then, as its last line, the
   admin's one-time enrollment link (`https://<your host>/enroll#token=...`). The link works
   once, for 7 days, and lets whoever holds it set up a passkey or password for the admin: treat
   it as a password. When the output goes to a file or a log rather than a terminal, the command
   warns on stderr that the log keeps the link. While an organization exists it refuses to
   create another and exits 2.

   If the link expires unused or is lost, make a new one for the same admin. It replaces the
   old link:

   ```sh
   docker compose -f deploy/compose/compose.production.yaml run --rm stepclimb \
     admin enrollment-link --org example --user admin@example.org
   ```

   `--user` takes the email, in any letter case, or the user's id. The command refuses (exit 2,
   nothing changed) a user who is not an Organization admin, unless you add
   `--admin-required-override`, and a user who can already sign in, unless you add `--force`.
   It exists only on the command line, for whoever has shell access to the host.

Without the `tls` profile the server is published on `127.0.0.1:8080` only, for a reverse proxy
on the same host. To serve HTTPS from the stack itself, see [HTTPS with Caddy](#https-with-caddy).

Every `docker compose` command on this file needs `SC_PUBLIC_URL`, `down` and `exec` included;
the `.env` file covers that. A command also needs the profiles the stack was started with: `down`
without `--profile tls` leaves Caddy running.

## Secret files

The example has no default passwords. Each secret is a file in the directory `SC_SECRETS_DIR`
names, mounted into the containers with Compose `secrets:`. Starting the stack while a file is
missing fails, naming the file. Keep that directory outside the checkout: a `git clean -x`, a
`rm -rf` of the checkout or a copy of it then cannot take the secrets along. Without
`SC_SECRETS_DIR` the stack reads `deploy/compose/secrets/`, which git and the image build
ignore.

| File | Holds |
| --- | --- |
| `postgres_password` | The PostgreSQL password of the user `stepclimb` |
| `database_url` | `postgres://stepclimb:<that password>@postgres:5432/stepclimb` |
| `encryption_keys` | The server's encryption key ring (see `STEPCLIMB_ENCRYPTION_KEYS` below) |
| `minio_root_user`, `minio_root_password` | MinIO's root credentials, with the `minio` profile only |

```sh
secrets="$HOME/stepclimb-secrets"
mkdir -m 700 "$secrets"
openssl rand -hex 32 > "$secrets/postgres_password"
printf 'postgres://stepclimb:%s@postgres:5432/stepclimb\n' \
  "$(cat "$secrets/postgres_password")" > "$secrets/database_url"
printf 'k1:%s\n' "$(openssl rand -base64 32)" > "$secrets/encryption_keys"
chmod 644 "$secrets"/*
echo "SC_SECRETS_DIR=$secrets" >> deploy/compose/.env
```

A hex password needs no escaping in the URL. A password with `@`, `/`, `?`, `&` or `#` must be
percent-encoded in `database_url`; the server refuses the URL otherwise.

**Permissions.** A file holding a secret should be readable by its owner only. Compose outside
Swarm cannot do that here: it bind-mounts each file as it is on the host and ignores a secret's
uid and mode, and the server and MinIO run as uid 65532, which cannot read a file only you can.
So the directory is owner-only (0700) and the files in it are 0644. The directory is the
protection: a copy of a file outside it is readable by every user on the host. `database_url`
repeats the PostgreSQL password; treat it as that password.

Keep a copy of `encryption_keys` apart from the dumps. A value the server stores encrypted under
the key ring cannot be read with any other ring, so a dump restored under new keys loses those
values.

## HTTPS with Caddy

The `tls` profile adds Caddy (`Caddyfile`), which serves `SC_DOMAIN` over HTTPS with an automatic
certificate, redirects HTTP to HTTPS and proxies to the server:

```sh
echo 'SC_DOMAIN=lms.example.org' >> deploy/compose/.env
docker compose -f deploy/compose/compose.production.yaml --profile tls up -d --build --wait
```

- Set `SC_PUBLIC_URL` to `https://` and the domain.
- Caddy publishes ports 80 and 443 (TCP, and 443 over UDP) on every address. Caddy without
  `SC_DOMAIN` stops with a message naming it.
- Certificates come from Let's Encrypt by default, which needs the domain's DNS pointing at the
  host and both ports reachable. `SC_CERT_ISSUER=internal` uses Caddy's own CA instead, for a host
  the internet cannot reach; browsers trust it only after its root certificate
  (`/data/caddy/pki/authorities/local/root.crt` in the `caddy_data` volume) is installed. The
  Caddyfile sets no ACME account email.
- The server keeps its `127.0.0.1:8080` port, reachable from the host only: a Compose profile
  cannot remove another service's port.

Caddy discards the forwarding headers a client sends and writes its own. It has the fixed address
`.2` on the `proxy` network it shares with the server (`10.201.73.0/24` unless `SC_PROXY_NETWORK`
changes the first three octets; change it if that range is in use on the host), and the server
trusts `X-Forwarded-For` from that address only (`STEPCLIMB_TRUSTED_PROXIES`).

The server rate-limits sign-in and token requests per client address, though the limits have
nothing to count until the sign-in and token routes arrive (none exists in M0). Caddy sees each client's
real address only where Docker passes it through: rootful Docker does for connections from other
hosts; rootless Docker does only with a RootlessKit port driver that keeps source addresses (see
Docker's rootless documentation). Otherwise every request arrives from the network gateway, `.1`,
and every client shares one budget. The same applies to a reverse proxy of your own on the host,
which reaches the server from `.1`; trusting it needs a Compose override that sets
`STEPCLIMB_TRUSTED_PROXIES`.

## MinIO

The `minio` profile adds MinIO on the internal network, with its data on the `minio` volume. Its
image is Chainguard's, pinned by digest (`docs/adr/0019-minio-image.md`). Create its two secret
files first, then name the profile:

```sh
secrets="$HOME/stepclimb-secrets"
printf 'stepclimb-media\n' > "$secrets/minio_root_user"
openssl rand -hex 24 > "$secrets/minio_root_password"
chmod 644 "$secrets"/minio_*
docker compose -f deploy/compose/compose.production.yaml --profile minio up -d --build --wait
```

The server stores course files on the `storage` volume (`STEPCLIMB_STORAGE_URL` is
`file:///var/lib/stepclimb/storage`), so MinIO is optional. To keep files in MinIO instead, set
`STEPCLIMB_STORAGE_URL=s3://<bucket>`, `STEPCLIMB_STORAGE_S3_ENDPOINT=http://minio:9000`,
`STEPCLIMB_STORAGE_S3_ALLOW_HTTP=true` and the two access key settings in a Compose override file,
and make the bucket before the server starts: `serve` writes, reads and deletes a probe object at
startup and stops when it cannot.

## Compose variables

These `SC_*` variables configure the Compose file, not the server. Set them in the environment or
in `deploy/compose/.env`. A test (`bin/test_compose_guide.py`, in `just ci`) fails when this table
and `compose.production.yaml` disagree on a name or a default.

| Variable | Default | What it sets |
| --- | --- | --- |
| `SC_PUBLIC_URL` | required | The address users open; becomes the server's `STEPCLIMB_PUBLIC_URL` |
| `SC_DOMAIN` | none | The domain Caddy serves; required with the `tls` profile |
| `SC_CERT_ISSUER` | `acme` | `acme` (Let's Encrypt) or `internal` (Caddy's own CA) |
| `SC_SECRETS_DIR` | `./secrets` | The directory holding the secret files: an absolute path, or one relative to `deploy/compose/` |
| `SC_HTTP_ADDRESS` | `127.0.0.1` | The host address the server is published on |
| `SC_HTTP_PORT` | `8080` | The host port the server is published on |
| `SC_PROXY_ADDRESS` | `0.0.0.0` | The host address Caddy is published on |
| `SC_PROXY_HTTP_PORT` | `80` | Caddy's HTTP port on the host |
| `SC_PROXY_HTTPS_PORT` | `443` | Caddy's HTTPS port on the host |
| `SC_PROXY_NETWORK` | `10.201.73` | The first three octets of the `proxy` network's /24 |
| `SC_BACKUP_AT` | `03:00` | The time of the nightly dump, UTC, as `HH:MM` |
| `SC_BACKUP_KEEP_DAYS` | `7` | How many days of dumps to keep, counted from the newest |
| `SC_IMAGE` | `stepclimb:local` | The tag of the server image built from this checkout |

## Server configuration

The server reads `STEPCLIMB_*` environment variables over an optional `stepclimb.toml`
(`docs/adr/0022-configuration-names-and-file.md`).

- Every setting has one variable and one top-level TOML key of the same name: `STEPCLIMB_BIND`
  and `bind`. A variable overrides the file, setting by setting.
- The file is read only when `--config <path>` or `STEPCLIMB_CONFIG` names it; there is no
  default path. Every value in it is a string in quotes, numbers included
  (`sign_in_rate_limit = "30"`), and a list is one comma-separated string.
- A secret also has a `_FILE` variable and a `_file` key naming a file whose contents, trimmed,
  are the value. A relative `_FILE` path is relative to the working directory; a relative `_file`
  path is relative to the directory holding `stepclimb.toml`. Setting a secret and its file
  variant is an error.
- An empty value counts as unset.
- An unknown `STEPCLIMB_*` variable or TOML key is an error, so a misspelled secret cannot start
  the server without it. So are a missing required value, a value that does not parse, an
  unreadable file and a TOML syntax error. The message names the variable, or the key with its
  file and line, and never repeats a secret or a `_FILE` path. The server exits with code 2
  before it connects to anything.

The production example sets `STEPCLIMB_DATABASE_URL_FILE`, `STEPCLIMB_ENCRYPTION_KEYS_FILE`,
`STEPCLIMB_PUBLIC_URL` and `STEPCLIMB_TRUSTED_PROXIES`, and the image sets `STEPCLIMB_BIND` to
`0.0.0.0:8080`. To set others, add an override file and name it after the production file. Paths
in it are relative to `deploy/compose/`:

```yaml
# deploy/compose/compose.override.local.yaml
services:
  stepclimb:
    environment:
      STEPCLIMB_SIGN_IN_RATE_LIMIT: "30"
      STEPCLIMB_CONFIG: /etc/stepclimb/stepclimb.toml
    volumes:
      - ./stepclimb.toml:/etc/stepclimb/stepclimb.toml:ro
```

```sh
docker compose -f deploy/compose/compose.production.yaml \
  -f deploy/compose/compose.override.local.yaml up -d --wait
```

### Configuration reference

A test (`every_setting_is_in_the_configuration_reference` in
`crates/stepclimb-server/src/config/tests.rs`) fails when a setting is missing here or its key,
default or secrecy is wrong.

| Variable | TOML key | Type | Default | Secret | Meaning |
| --- | --- | --- | --- | --- | --- |
| `STEPCLIMB_CONFIG` | none (`--config` on the command line wins) | path | none | no | The configuration file to read |
| `STEPCLIMB_BIND` | `bind` | socket address | `127.0.0.1:8080` | no | The address and port the server listens on |
| `STEPCLIMB_LOG` | `log` | log filter | `info` | no | The filter for the JSON logs on stderr, such as `stepclimb_server=debug,info` |
| `STEPCLIMB_DATABASE_URL`, `STEPCLIMB_DATABASE_URL_FILE` | `database_url`, `database_url_file` | PostgreSQL URL | required | yes | `postgres://` or `postgresql://`; only the query parameters sqlx reads are accepted |
| `STEPCLIMB_ENCRYPTION_KEYS`, `STEPCLIMB_ENCRYPTION_KEYS_FILE` | `encryption_keys`, `encryption_keys_file` | key ring | required | yes | `<id>:<key>` entries separated by commas or whitespace; the id is 1 to 32 of `A-Z a-z 0-9 . _ -`, the key 32 bytes in padded base64 (`openssl rand -base64 32`). The first key encrypts and every key decrypts: rotate by putting a new key first and keeping the old one |
| `STEPCLIMB_PUBLIC_URL` | `public_url` | origin | required | no | The `http` or `https` origin users open, without a path. An `https` one turns on HSTS |
| `STEPCLIMB_ALLOWED_ORIGINS` | `allowed_origins` | comma-separated origins | none | no | Further origins a state-changing request's `Origin` header may name, such as a development server |
| `STEPCLIMB_TRUSTED_PROXIES` | `trusted_proxies` | comma-separated addresses or CIDR ranges | none | no | Reverse proxies whose `X-Forwarded-For` names the client address for the rate limits. A range may be no wider than /8 (IPv4) or /32 (IPv6) and must hold proxies only |
| `STEPCLIMB_SIGN_IN_RATE_LIMIT` | `sign_in_rate_limit` | whole number, 1 or more | `20` | no | Sign-in requests per minute from one client address |
| `STEPCLIMB_TOKEN_RATE_LIMIT` | `token_rate_limit` | whole number, 1 or more | `60` | no | `/oauth/token` and `/oauth/revoke` requests per minute from one client address |
| `STEPCLIMB_FAILED_SIGN_IN_LIMIT` | `failed_sign_in_limit` | whole number, 1 or more | `10` | no | Failed password or TOTP checks per account per 15 minutes |
| `STEPCLIMB_ALLOW_INSECURE_PROVIDERS` | `allow_insecure_providers` | `true` or `false` | `false` | no | Lets the server fetch `http` and loopback URLs for identity providers and the admission webhook. For tests and local development only: it lets an organization admin make the server call services on its own host |
| `STEPCLIMB_ROLE` | `role` | `all`, `web` or `worker` | `all` | no | What `stepclimb serve` runs: `all` the HTTP application and the job worker, `web` the HTTP application only, `worker` the job worker with only `/healthz` and `/readyz` on the bind address. `serve --role` overrides it |
| `STEPCLIMB_WORKER_CONCURRENCY` | `worker_concurrency` | whole number, 1 to 64 | `4` | no | Jobs one `all` or `worker` process runs at once |
| `STEPCLIMB_STORAGE_URL` | `storage_url` | URL | none | no | Where course files are stored: `file:///<absolute directory>` (the directory must exist) or `s3://<bucket>[/<prefix>]`. Unset, the server starts, logs a warning, and uploads and file downloads answer 503 `storage.not_configured`. When set, `serve` writes, reads and deletes an object under `<prefix>/.probe/` before it listens and stops if any step fails |
| `STEPCLIMB_STORAGE_S3_ENDPOINT` | `storage_s3_endpoint` | origin | none | no | The origin of an S3-compatible store, such as `http://minio:9000`; unset for AWS S3. The `STEPCLIMB_STORAGE_S3_*` settings are refused without an `s3://` storage URL |
| `STEPCLIMB_STORAGE_S3_REGION` | `storage_s3_region` | region name | `us-east-1` | no | The bucket's region |
| `STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID` | `storage_s3_access_key_id` | string | required for `s3://` | no | The access key the store issued. The server reads no credentials from the environment or an instance profile |
| `STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY`, `STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY_FILE` | `storage_s3_secret_access_key`, `storage_s3_secret_access_key_file` | string | required for `s3://` | yes | The secret of that access key |
| `STEPCLIMB_STORAGE_S3_ALLOW_HTTP` | `storage_s3_allow_http` | `true` or `false` | `false` | no | Lets the endpoint be plain `http`, for a store on the same host or a private network such as the Compose `backend` network |

The three limits have nothing to count until the sign-in and token routes arrive; none exists in
M0.

Each `serve` process holds up to 10 database connections for HTTP, and an `all` or `worker`
process another `worker_concurrency` + 1 for the job worker (11 with the default 4), so up to
10 + `worker_concurrency` + 1 in all. PostgreSQL allows 100 connections by default
(`max_connections`); when you run several processes against one database, keep their sum, plus
what backups and administration use, under it. A `web` process opens no job connections.

The `stepclimb` service in `compose.production.yaml` sets `stop_grace_period: 40s`. On SIGTERM
the job worker stops claiming and waits up to 30 s for the jobs it is running; Docker's default
of 10 s would kill the process before then. A job still running at 30 s is left to its lease,
which runs out so another worker takes it.

Periodic jobs, such as the sign-in flow cleanup every 10 minutes and the hourly pruning of dead
jobs, run only in `all` and `worker` processes. Keep at least one of those running; a deployment
of `web` processes alone runs no jobs.


### Sign-in providers

OpenID Connect and OAuth 2 sign-in providers can be declared in `stepclimb.toml` as
`[[providers]]` tables; they have no variable. Unlike the settings above, their values use TOML's
own types: `scopes` is an array and `enabled` a boolean.

```toml
[[providers]]
kind = "oauth2"
key = "vatsim"
label = "Sign in with VATSIM"
authorize = "https://auth.vatsim.net/oauth/authorize"
token = "https://auth.vatsim.net/oauth/token"
userinfo = "https://auth.vatsim.net/api/user"
scopes = ["full_name", "vatsim_details", "email"]
client_id = "<your client id>"
client_secret_file = "/run/secrets/vatsim_client_secret"

[providers.claims]
subject = "/data/cid"
given_name = "/data/personal/name_first"
family_name = "/data/personal/name_last"
email = "/data/personal/email"
attributes.rating = "/data/vatsim/rating/short"
attributes.subdivision = "/data/vatsim/subdivision/id"
```

Each entry has `kind` (`oidc` or `oauth2`), `key` (1 to 32 lowercase letters, digits and
hyphens, part of the redirect URI `<public URL>/auth/<key>/callback` you register at the
provider), `label`, `enabled` (default `true`), and exactly one of `client_secret` and
`client_secret_file`. The other members are the provider's config: for `oidc`, `issuer`,
`client_id`, `scopes`, `claims`, `userinfo` and `allow_local_passkeys`; for `oauth2`, the three
URLs, `client_id`, `scopes`, `pkce`, `claims` and `allow_local_passkeys`
(`docs/specs/m0/05-oidc.md` B1, `docs/specs/m0/06-oauth2-and-vatsim-preset.md` B4). On an
instance with more than one organization, `org = "<slug>"` names the provider's organization. An entry
cannot set `admission_rules`; those are managed through the admin API and kept across restarts.

A server built with the `vatsim` feature, as the release image is, has a VATSIM Connect preset
that supplies the kind, key, label, URLs, scopes and claims above, so the entry needs only the
client:

```toml
[[providers]]
preset = "vatsim"
client_id = "<your client id>"
client_secret_file = "/run/secrets/vatsim_client_secret"
```

Any member the entry names replaces the preset's, `claims` as a whole. To use VATSIM's
development environment, set `authorize`, `token` and `userinfo` to the same paths on
`https://auth-dev.vatsim.net`. The database row keeps only what differs from the preset, so a
release that corrects the preset reaches the provider on its next read. VATSIM's documentation
does not say whether Connect accepts PKCE, which the preset sends; if sign-in fails at the token
step, `pkce = false` turns it off, at the cost of the protection against a stolen authorization
code (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` Q3).

- The file owns the providers it declares. Every start of `serve` writes them to their
  organization by `key`, taking over a provider of that key created any other way, and disables
  (never deletes) one that is no longer in the file, so its users' sign-in links are kept.
- The client secret is never stored in the database: the server reads it from the file at
  startup and keeps it in memory. Every server of an instance must read the same file: a server whose
  file differs from the one that last wrote a provider refuses to sign in with it.
- While people sign in with a provider, the file cannot take it over from the admin API or
  change its kind, issuer, endpoint hosts or subject claim: the server stops with exit 2.
- An invalid entry stops the server with exit code 2, naming the entry's position, its key and
  the member, and never the secret; `check-config` reports the same errors, and prints the
  providers as `key (kind)`. An `org` that names no organization, or a missing `org` on an
  instance with several, is found only once `serve` has connected; it also exits 2.
- On a fresh instance there is no organization to add them to: `serve` logs a warning and starts
  without them. Restart it after `admin create`.

## Commands

The image's entry point is the `stepclimb` binary, so Compose runs a command by naming it after
the service. `--config <path>` works with every command.

| Command | What it does | Exit codes |
| --- | --- | --- |
| `serve` | The default: applies pending migrations, writes the `[[providers]]` of `stepclimb.toml`, then serves HTTP until SIGINT or SIGTERM | 0 on shutdown; 2 for a provider entry that fits no organization; 1 when the database, a migration or the listener fails |
| `migrate` | Applies pending migrations and exits | 0; 1 on a database or migration failure |
| `check-config` | Checks the configuration without connecting to anything and prints its non-secret values | 0 when valid; 2 when refused |
| `admin create` | Applies migrations, then creates the organization and its first admin in one transaction | 0; 2 for a refused value or an existing organization; 1 on a database failure |
| `admin enrollment-link` | Applies migrations, then makes a new one-time enrollment link for an organization's admin and prints it | 0; 2 for an unknown organization or user, a user who is not an Organization admin or can already sign in; 1 on a database failure |
| `healthcheck` | Asks `/readyz` on the server's `STEPCLIMB_BIND` port; the container health check | 0 when it answers 200; 1 otherwise |

Every command exits 2 when the configuration is refused, `serve` also when a `[[providers]]`
entry does not fit the instance's organizations.

```sh
docker compose -f deploy/compose/compose.production.yaml run --rm --no-deps stepclimb check-config
docker compose -f deploy/compose/compose.production.yaml run --rm stepclimb migrate
```

`--no-deps` keeps `check-config` from starting PostgreSQL, which it does not need.

## Health probes

| Probe | Answers |
| --- | --- |
| `GET /healthz` | 200 `{"status":"ok"}` whenever the process runs. Restart the server when it stops answering |
| `GET /readyz` | 200 `{"status":"ok"}` when the database answers `SELECT 1` within 2 seconds, otherwise 503 `{"status":"unavailable"}`. Send traffic only while it answers 200 |

The server applies migrations before it listens, so `/readyz` never answers 200 against an
outdated schema. The image has no shell or `curl`; its Compose health check runs
`stepclimb healthcheck` (`docs/adr/0023-container-healthcheck-subcommand.md`), and Caddy starts
once the server reports healthy.

## Volumes and backups

| Volume | Holds | Back it up |
| --- | --- | --- |
| `postgres` | The database | Yes, through the nightly dumps |
| `backups` | The nightly dumps | Copy them off the host |
| `caddy_data` | Caddy's certificates and keys (`tls` profile) | Keep it: without it, a restart asks the CA for new certificates |
| `caddy_config` | Caddy's saved configuration (`tls` profile) | No: Caddy rebuilds it from the Caddyfile |
| `storage` | Course files (`STEPCLIMB_STORAGE_URL`) | Yes, with the database: its `files` rows name the objects here. The nightly dumps do not include it |
| `minio` | Object storage (`minio` profile) | Yes, when the server stores files there |

Compose prefixes each name with the project, `stepclimb-production`. `docker compose down` keeps
the volumes; `down --volumes` deletes them, the database included.

The `backup` service dumps the database with `pg_dump` every night at `SC_BACKUP_AT` (UTC) into
the `backups` volume, as `stepclimb-<UTC time>.dump` in `pg_dump`'s custom format. It keeps every
dump taken within `SC_BACKUP_KEEP_DAYS` days of the newest one, so nights without a dump delete
nothing and a dump taken by hand never pushes a nightly one out. A failed dump is logged and
retried the next night.

Copy the dumps off the host, and never into this checkout: a dump holds every organization's
data, and one on the same disk as the database is lost with it. To take a dump now and copy all
of them to a directory outside the repository:

```sh
docker compose -f deploy/compose/compose.production.yaml exec backup stepclimb-backup now
mkdir -m 700 -p ~/stepclimb-backups
docker compose -f deploy/compose/compose.production.yaml cp backup:/backups/. ~/stepclimb-backups
```

## Restore

A dump restores into a new, empty stack (a fresh `postgres` volume), before the server first
starts. The restore refuses a database that already has tables.

```sh
docker compose -f deploy/compose/compose.production.yaml up -d --wait postgres
docker compose -f deploy/compose/compose.production.yaml run --rm \
  -v ~/stepclimb-backups:/restore:ro backup stepclimb-backup restore /restore/<file>.dump
docker compose -f deploy/compose/compose.production.yaml up -d --build --wait
```

Use the same `encryption_keys` file the dumped server had. `just compose-ops-check` takes a dump
of a stack with data, restores it into a fresh one and checks that the server turns healthy with
the data present.

## Upgrades

1. Take a dump and copy it off the host (above).
2. Check out the new release in this repository.
3. Rebuild and restart: `docker compose -f deploy/compose/compose.production.yaml up -d --build --wait`.

The new server applies its forward-only migrations as it starts. There is no downgrade: going
back means restoring the dump from step 1 into a fresh stack. Once release images are published
(not in M0), the upgrade becomes `pull` then `up -d`.
