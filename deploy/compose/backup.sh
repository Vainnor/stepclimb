#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-or-later
# Nightly PostgreSQL dumps for compose.production.yaml's `backup` service, which runs it in the
# postgres image so pg_dump matches the server's major version.
#
#   stepclimb-backup daemon          dump every day at SC_BACKUP_AT (UTC), keeping the dumps of
#                                    the last SC_BACKUP_KEEP_DAYS days (the service's command)
#   stepclimb-backup now             one dump now, then the same pruning
#   stepclimb-backup restore FILE    restore FILE into the database, which must be empty
#
# Dumps are pg_dump's custom format, /backups/stepclimb-<UTC time>.dump, readable by root only.
# A dump is written under a .partial name and renamed when pg_dump succeeds, so a dump that
# failed halfway is never mistaken for a good one.
#
# Pruning keeps every dump taken within SC_BACKUP_KEEP_DAYS days of the newest dump, by the time
# in its name. Ages count from the newest dump, so nights without a dump delete nothing and extra
# dumps taken by hand never push a nightly one out. A name dated after the clock (a clock that
# jumped back, a copied file) is logged, and ages then count from the clock instead, so a dump
# from the future cannot age every new dump out. The dump just written is never deleted.
set -eu

backups=/backups
at=${SC_BACKUP_AT:-03:00}
keep_days=${SC_BACKUP_KEEP_DAYS:-7}

log() { echo "stepclimb-backup: $*" >&2; }
die() {
    log "$*"
    exit 1
}

check_settings() {
    echo "$at" | grep -Eq '^([01][0-9]|2[0-3]):[0-5][0-9]$' \
        || die "SC_BACKUP_AT must be a UTC time as HH:MM, for example 03:00"
    echo "$keep_days" | grep -Eq '^[1-9][0-9]*$' \
        || die "SC_BACKUP_KEEP_DAYS must be a whole number of days of at least 1, for example 7"
    [ -r /run/secrets/postgres_password ] || die "the secret postgres_password is not mounted"
    PGPASSWORD=$(cat /run/secrets/postgres_password)
    export PGPASSWORD PGHOST=postgres PGUSER=stepclimb PGDATABASE=stepclimb
}

dump() {
    umask 077
    name="stepclimb-$(date -u +%Y%m%dT%H%M%SZ).dump"
    pg_dump --format=custom --file="$backups/$name.partial" \
        || { rm -f "$backups/$name.partial"; die "pg_dump failed; no dump written"; }
    mv "$backups/$name.partial" "$backups/$name" || die "could not rename $name.partial"
    log "wrote $backups/$name"
    prune "$backups/$name"
}

# Every dump in $backups, oldest first: names hold the UTC time, so they sort by it. Files with
# any other name are left alone.
dumps() {
    find "$backups" -maxdepth 1 -name 'stepclimb-*.dump' \
        | grep -E '/stepclimb-[0-9]{8}T[0-9]{6}Z\.dump$' | sort
}

# The UTC time in a dump's name, in seconds since the epoch.
dump_time() {
    stamp=$(basename "$1" | sed -E 's/^stepclimb-(....)(..)(..)T(..)(..)(..)Z\.dump$/\1-\2-\3 \4:\5:\6/')
    date -u -d "$stamp" +%s
}

# Delete the dumps more than $keep_days days older than the newest, or than now if the newest is
# dated later; never $1, the dump just written.
prune() {
    newest=$(dumps | tail -n 1)
    [ -n "$newest" ] || return 0
    reference=$(dump_time "$newest")
    now=$(date -u +%s)
    if [ "$reference" -gt "$now" ]; then
        log "$newest is dated in the future; counting ages from now instead"
        reference=$now
    fi
    cutoff=$((reference - keep_days * 86400))
    dumps | while read -r old; do
        [ "$old" != "$1" ] || continue
        [ "$(dump_time "$old")" -lt "$cutoff" ] || continue
        rm -f "$old"
        log "removed $old (more than $keep_days days old)"
    done
}

seconds_until_next_run() {
    now=$(date -u +%s)
    next=$(date -u -d "$(date -u +%Y-%m-%d) $at" +%s)
    [ "$next" -gt "$now" ] || next=$((next + 86400))
    echo $((next - now)) "$(date -u -d "@$next" +%Y-%m-%dT%H:%M:%SZ)"
}

daemon() {
    while :; do
        set -- $(seconds_until_next_run)
        log "next dump at $2"
        sleep "$1"
        # A failed dump is logged and retried the next night; the daemon keeps running.
        (dump) || log "the dump at $2 failed"
    done
}

restore() {
    [ $# -eq 1 ] || die "usage: stepclimb-backup restore FILE"
    [ -r "$1" ] || die "cannot read the dump $1"
    tables=$(psql --no-psqlrc --tuples-only --no-align \
        --command="SELECT count(*) FROM pg_tables WHERE schemaname = 'public'")
    [ "$tables" = 0 ] \
        || die "the database already has $tables tables; restore only into a new, empty stack"
    pg_restore --single-transaction --no-owner --dbname="$PGDATABASE" "$1"
    log "restored $1"
}

check_settings
case "${1:-}" in
daemon) daemon ;;
now) dump ;;
restore)
    shift
    restore "$@"
    ;;
*) die "usage: stepclimb-backup daemon | now | restore FILE" ;;
esac
