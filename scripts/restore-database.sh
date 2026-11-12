#!/usr/bin/env bash
# Restores a dump made by backup-database.sh into an empty database.
#
#   RESTORE_DATABASE_URL   The database to restore into. It must hold no
#                          tables. On Neon, the direct endpoint.
#   BACKUP_AGE_IDENTITY    A file holding the age private key.
#
#   scripts/restore-database.sh FILE
#
# After the tables are back, every account's epoch is raised, by
# raise-epochs.sh, to a number no device has seen. A device that sees a new
# epoch forgets the change numbers it has seen and merges everything again,
# so whatever the devices hold that the dump does not is sent again. Nothing
# a device holds is lost by a restore.
#
# What a restore does undo is on the server alone: see "After a restore" in
# docs/DEPLOY.md before pointing the server at the restored database.
#
# pg_restore and psql must be at least as new as the dump (Postgres 18).
# PG_RESTORE, PSQL and AGE name the programs to run when they are not on the
# PATH; each may be a command with arguments.
set -euo pipefail

: "${RESTORE_DATABASE_URL:?set RESTORE_DATABASE_URL to the empty database to restore into}"
: "${BACKUP_AGE_IDENTITY:?set BACKUP_AGE_IDENTITY to the file holding the age private key}"
file="${1:?name the dump to restore}"
[ -s "$file" ] || { echo "$file is missing or empty" >&2; exit 1; }

here="$(cd "$(dirname "$0")" && pwd)"
read -r -a pg_restore <<< "${PG_RESTORE:-pg_restore}"
read -r -a psql <<< "${PSQL:-psql}"
read -r -a age <<< "${AGE:-age}"

sql() {
  "${psql[@]}" --no-psqlrc --quiet --tuples-only --no-align \
    --set ON_ERROR_STOP=1 --dbname="$RESTORE_DATABASE_URL" --command "$1"
}

tables="$(sql "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'" | tr -d '[:space:]')"
if [ "$tables" != "0" ]; then
  echo "the database already holds $tables tables; restore into an empty one" >&2
  exit 1
fi

# One transaction: the database ends up restored or untouched.
"${age[@]}" --decrypt --identity "$BACKUP_AGE_IDENTITY" < "$file" \
  | "${pg_restore[@]}" --no-owner --no-privileges --exit-on-error \
      --single-transaction --dbname="$RESTORE_DATABASE_URL"

raised="$("$here/raise-epochs.sh")"

echo "restored $file:"
sql "SELECT '  ' || (SELECT count(*) FROM accounts) || ' accounts, '
         || (SELECT count(*) FROM devices) || ' devices, '
         || (SELECT count(*) FROM notes) || ' notes; schema version '
         || (SELECT max(version) FROM _sqlx_migrations)"
echo "  $raised"
