#!/bin/sh
# Isolated backup and restore of the current System slice (Roadmap 1.5.3).
#
# Run on an approved PostgreSQL/PostGIS host as its administrative OS user,
# with that server's own pg_dump/pg_restore. Databases hold synthetic test data.
#
#   system-restore.sh backup SOURCE_DB DUMP_FILE
#   system-restore.sh restore DUMP_FILE SOURCE_DB TARGET_DB INSPECTION_ROLE
#
# The restored target is an inspection clone, not a serving database: writes are
# refused, only INSPECTION_ROLE (and administrators) may connect, and nothing
# here activates it for normal serving, export or delivery.
set -eu

host=${PGHOST:-/var/run/postgresql}

fail() {
    printf 'system-restore: %s\n' "$1" >&2
    exit 1
}

# Plain lowercase identifiers only, so names are never quoted or interpolated.
name_ok() {
    case "$1" in
        '' | *[!a-z0-9_]*) return 1 ;;
        *) return 0 ;;
    esac
}

sql() {
    psql -X -w -qAt --host="$host" --dbname="$1" --set=ON_ERROR_STOP=1 --command="$2"
}

case "${1:-}" in
    backup)
        [ $# -eq 3 ] || fail "usage: backup SOURCE_DB DUMP_FILE"
        source_db=$2
        dump=$3
        name_ok "$source_db" || fail "invalid source database name"
        [ ! -e "$dump" ] || fail "refusing to overwrite an existing dump file"
        # One consistent snapshot of every table: the backup's recovery point.
        pg_dump --host="$host" --no-password --format=custom --file="$dump" "$source_db"
        printf 'Backed up %s.\n' "$source_db"
        ;;
    restore)
        [ $# -eq 5 ] || fail "usage: restore DUMP_FILE SOURCE_DB TARGET_DB INSPECTION_ROLE"
        dump=$2
        source_db=$3
        target=$4
        role=$5
        for name in "$source_db" "$target" "$role"; do
            name_ok "$name" || fail "invalid database or role name"
        done
        [ "$target" != "$source_db" ] || fail "restore target is the source database"
        [ -f "$dump" ] || fail "dump file is missing"
        existing=$(sql postgres "SELECT count(*) FROM pg_database WHERE datname = '$target'")
        [ "$existing" = 0 ] || fail "restore target already exists; refusing to overwrite it"
        createdb --host="$host" --no-password --template=template0 "$target"
        pg_restore --host="$host" --no-password --exit-on-error --single-transaction \
            --dbname="$target" "$dump"
        # Inspection only: no request can add resources, audit, retries or outgoing work.
        sql "$target" "REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON ALL TABLES IN SCHEMA public FROM $role" # CLONE_READ_ONLY
        sql postgres "ALTER DATABASE $target SET default_transaction_read_only = on" # CLONE_READ_ONLY
        # Only the named inspection role and administrators may connect.
        sql postgres "REVOKE CONNECT ON DATABASE $target FROM PUBLIC" # CLONE_CONNECT
        sql postgres "GRANT CONNECT ON DATABASE $target TO $role" # CLONE_CONNECT
        printf 'Restored into isolated inspection database %s.\n' "$target"
        ;;
    *)
        fail "usage: backup SOURCE_DB DUMP_FILE | restore DUMP_FILE SOURCE_DB TARGET_DB INSPECTION_ROLE"
        ;;
esac
