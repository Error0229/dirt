#!/usr/bin/env bash
# Dumps the server's database into one encrypted file.
#
#   BACKUP_DATABASE_URL    The database to dump. On Neon, the direct endpoint.
#   BACKUP_AGE_RECIPIENT   The age public key to encrypt to ("age1...").
#
#   scripts/backup-database.sh [FILE]
#
# FILE defaults to dirt-<UTC time>.dump.age in the current folder.
#
# The dump holds every account's encrypted notes, its device records and its
# e-mail address. It is encrypted here, before it goes anywhere, to a key
# whose private half is kept off every server. Where the file is then stored
# does not have to be trusted with what is in it.
#
# pg_dump must be at least as new as the server (Postgres 18). PG_DUMP and
# AGE name the programs to run when they are not on the PATH; each may be a
# command with arguments.
#
# docs/DEPLOY.md, "Backups", says how this is scheduled and how to restore.
set -euo pipefail

: "${BACKUP_DATABASE_URL:?set BACKUP_DATABASE_URL to the database to dump}"
: "${BACKUP_AGE_RECIPIENT:?set BACKUP_AGE_RECIPIENT to the age public key to encrypt to}"

read -r -a pg_dump <<< "${PG_DUMP:-pg_dump}"
read -r -a age <<< "${AGE:-age}"

file="${1:-dirt-$(date -u +%Y%m%dT%H%M%SZ).dump.age}"
partial="$file.partial"
trap 'rm -f "$partial"' EXIT

# Request counts and sign-in nonces are worth nothing an hour later. Their
# tables are dumped, empty.
"${pg_dump[@]}" \
  --format=custom --no-owner --no-privileges \
  --exclude-table-data=request_counts --exclude-table-data=nonces \
  --dbname="$BACKUP_DATABASE_URL" \
  | "${age[@]}" --recipient "$BACKUP_AGE_RECIPIENT" > "$partial"

if [ ! -s "$partial" ]; then
  echo "the dump is empty; nothing was saved" >&2
  exit 1
fi
mv "$partial" "$file"
echo "saved $file ($(wc -c < "$file" | tr -d ' ') bytes)"
