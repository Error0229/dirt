#!/usr/bin/env bash
# Tries backup-database.sh and restore-database.sh against a real Postgres:
# makes a database with the server's schema and a little data, dumps it,
# restores the dump into an empty database and compares the two.
#
#   DATABASE_URL   Any database on a server where this user may create
#                  databases, as for the server's tests.
#
#   scripts/check-backup.sh
#
# CI runs this, so that a change to the schema that the scripts cannot carry
# is found before a restore is ever needed. It needs pg_dump, pg_restore and
# psql at least as new as the server, age, and cargo.
set -euo pipefail

: "${DATABASE_URL:?set DATABASE_URL as for the tests of the server}"
here="$(cd "$(dirname "$0")" && pwd)"
server="${DATABASE_URL%/*}"
source_db="dirt_backup_check_source"
target_db="dirt_backup_check_target"
work="$(mktemp -d)"

admin() { psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname="$DATABASE_URL" "$@"; }
in_db() { psql --no-psqlrc --quiet --tuples-only --no-align --set ON_ERROR_STOP=1 --dbname="$server/$1" --command "$2"; }
drop_both() {
  admin --command "DROP DATABASE IF EXISTS $source_db" --command "DROP DATABASE IF EXISTS $target_db" 2> /dev/null
}
finish() { drop_both || true; rm -r "$work"; }
trap finish EXIT

drop_both
admin --command "CREATE DATABASE $source_db" --command "CREATE DATABASE $target_db"
MIGRATE_DATABASE_URL="$server/$source_db" cargo run --quiet -p dirt-api -- migrate

psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname="$server/$source_db" <<'SQL'
INSERT INTO accounts (id, clerk_issuer, clerk_user_id, email, change_counter, epoch,
                      key_version, key_established, note_count, bytes_used, old_key_notes,
                      purged_count)
VALUES ('01932aaa-0000-7000-8000-000000000001', 'https://issuer.example', 'user_1',
        'someone@example.com', 4, 2, 2, true, 3, 300007, 1, 1);
INSERT INTO devices (id, account_id, name, public_key, token_hash, status, revoked_at)
VALUES ('01932aaa-0000-7000-8000-000000000011', '01932aaa-0000-7000-8000-000000000001',
        'Laptop', decode(repeat('07', 32), 'hex'), '\x01', 'approved', NULL),
       ('01932aaa-0000-7000-8000-000000000012', '01932aaa-0000-7000-8000-000000000001',
        'Phone', decode(repeat('08', 32), 'hex'), '\x02', 'pending', NULL),
       ('01932aaa-0000-7000-8000-000000000013', '01932aaa-0000-7000-8000-000000000001',
        'Old tablet', decode(repeat('09', 32), 'hex'), '\x03', 'revoked', now());
-- One blob is large, so that the dump holds more than one page of it.
INSERT INTO notes (account_id, note_id, change_number, key_version, format_version, ciphertext)
VALUES ('01932aaa-0000-7000-8000-000000000001', '01932aaa-0000-7000-8000-000000000021', 1, 1, 1, '\x00010203'),
       ('01932aaa-0000-7000-8000-000000000001', '01932aaa-0000-7000-8000-000000000022', 2, 2, 1,
        decode(repeat('ab', 300000), 'hex')),
       ('01932aaa-0000-7000-8000-000000000001', '01932aaa-0000-7000-8000-000000000023', 3, 2, 1, '\x0a0b0c'),
       -- A purged note: its row, with no blob.
       ('01932aaa-0000-7000-8000-000000000001', '01932aaa-0000-7000-8000-000000000024', 4, 2, 1, NULL);
INSERT INTO device_keys (device_id, key_version, wrapped)
VALUES ('01932aaa-0000-7000-8000-000000000011', 2, '\x1122');
INSERT INTO enrolment_messages (device_id, seq, sender_id, body)
VALUES ('01932aaa-0000-7000-8000-000000000012', 1, '01932aaa-0000-7000-8000-000000000012', '\x33');
INSERT INTO enrolment_keys (device_id, approver_id, key_version, wrapped)
VALUES ('01932aaa-0000-7000-8000-000000000012', '01932aaa-0000-7000-8000-000000000011', 2, '\x44');
INSERT INTO recovery_keys (account_id, key_version, wrapped, claim_hash)
VALUES ('01932aaa-0000-7000-8000-000000000001', 2, '\x55', decode(repeat('66', 32), 'hex'));
INSERT INTO nonces (nonce_hash, expires_at) VALUES ('\x77', now());
INSERT INTO request_counts (account_id, window_start, requests)
VALUES ('01932aaa-0000-7000-8000-000000000001', now(), 5);
SQL

age-keygen --output "$work/key.txt" 2> /dev/null
recipient="$(age-keygen -y "$work/key.txt")"
age-keygen --output "$work/another-key.txt" 2> /dev/null

BACKUP_DATABASE_URL="$server/$source_db" BACKUP_AGE_RECIPIENT="$recipient" \
  "$here/backup-database.sh" "$work/check.dump.age"

if grep --quiet --text "someone@example.com" "$work/check.dump.age"; then
  echo "FAILED: the dump can be read without the key" >&2
  exit 1
fi

# The wrong key restores nothing and leaves the database empty.
if RESTORE_DATABASE_URL="$server/$target_db" BACKUP_AGE_IDENTITY="$work/another-key.txt" \
  "$here/restore-database.sh" "$work/check.dump.age" > /dev/null 2>&1; then
  echo "FAILED: a dump was restored with a key it was not encrypted to" >&2
  exit 1
fi
left="$(in_db "$target_db" "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'")"
if [ "$left" != "0" ]; then
  echo "FAILED: a restore that failed left $left tables behind" >&2
  exit 1
fi

RESTORE_DATABASE_URL="$server/$target_db" BACKUP_AGE_IDENTITY="$work/key.txt" \
  "$here/restore-database.sh" "$work/check.dump.age"

# Everything but the epoch, which the restore raises, and the two tables
# that are not carried.
contents="
  SELECT 'accounts', md5(string_agg(md5(row(id, clerk_issuer, clerk_user_id, email, change_counter,
           key_version, key_established, note_count, bytes_used, old_key_notes, purged_count,
           reset_requested_at, reset_requested_by, created_at)::text), '' ORDER BY id)) FROM accounts
  UNION ALL SELECT 'devices', md5(string_agg(md5(d::text), '' ORDER BY id)) FROM devices d
  UNION ALL SELECT 'notes', md5(string_agg(md5(n::text), '' ORDER BY note_id)) FROM notes n
  UNION ALL SELECT 'device_keys', md5(string_agg(md5(k::text), '' ORDER BY device_id, key_version)) FROM device_keys k
  UNION ALL SELECT 'enrolment_messages', md5(string_agg(md5(m::text), '' ORDER BY device_id, seq)) FROM enrolment_messages m
  UNION ALL SELECT 'enrolment_keys', md5(string_agg(md5(e::text), '' ORDER BY device_id)) FROM enrolment_keys e
  UNION ALL SELECT 'recovery_keys', md5(string_agg(md5(r::text), '' ORDER BY account_id)) FROM recovery_keys r
  UNION ALL SELECT 'migrations', md5(string_agg(version::text || encode(checksum, 'hex'), '' ORDER BY version)) FROM _sqlx_migrations
  ORDER BY 1"
before="$(in_db "$source_db" "$contents")"
after="$(in_db "$target_db" "$contents")"
if [ -z "$before" ] || [ "$before" != "$after" ]; then
  echo "FAILED: the restored database differs from the one that was dumped" >&2
  diff <(echo "$before") <(echo "$after") >&2 || true
  exit 1
fi

# The restored epoch has to be one no device has seen: well past the one
# that was dumped, and different each time the same dump is restored.
first="$(in_db "$target_db" "SELECT epoch FROM accounts")"
if [ "$first" -lt 1002 ]; then
  echo "FAILED: the restore should raise the epoch from 2 by at least a thousand, and it is $first" >&2
  exit 1
fi
carried="$(in_db "$target_db" "SELECT (SELECT count(*) FROM nonces) + (SELECT count(*) FROM request_counts)")"
if [ "$carried" != "0" ]; then
  echo "FAILED: nonces or request counts were carried into the restored database" >&2
  exit 1
fi

# The server's own view of the schema: nothing left to migrate.
MIGRATE_DATABASE_URL="$server/$target_db" cargo run --quiet -p dirt-api -- migrate > /dev/null

# The same dump restored a second time. Devices that synced after the first
# restore hold its epoch, and must be able to tell the second from it.
admin --command "DROP DATABASE $target_db" --command "CREATE DATABASE $target_db"
sleep 2
RESTORE_DATABASE_URL="$server/$target_db" BACKUP_AGE_IDENTITY="$work/key.txt" \
  "$here/restore-database.sh" "$work/check.dump.age" > /dev/null
second="$(in_db "$target_db" "SELECT epoch FROM accounts")"
if [ "$second" -le "$first" ]; then
  echo "FAILED: two restores of one dump gave epochs $first and $second; the second must be higher" >&2
  exit 1
fi

# After a restore by other means, such as the host's own history, the
# epochs are raised by hand with the same script.
RESTORE_DATABASE_URL="$server/$target_db" "$here/raise-epochs.sh" > /dev/null
third="$(in_db "$target_db" "SELECT epoch FROM accounts")"
if [ "$third" -le "$second" ]; then
  echo "FAILED: raising the epochs again left them at $third, no higher than $second" >&2
  exit 1
fi

echo "the dump restores to the same contents; epochs 2 -> $first, then $second, then $third"
