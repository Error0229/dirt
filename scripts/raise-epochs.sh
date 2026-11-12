#!/usr/bin/env bash
# Raises every account's epoch. Run it whenever the server's database has
# gone back in time, by whatever means: Neon's history, or a dump (which
# restore-database.sh does by calling this).
#
#   RESTORE_DATABASE_URL   The database that was restored. On Neon, the
#                          direct endpoint.
#
#   scripts/raise-epochs.sh
#
# A device remembers the epoch it last saw and the change numbers it has
# seen under it. When the epoch it is told differs, it forgets those numbers
# and merges everything again, which sends whatever it holds that the
# restored database does not. If the epoch is the one it remembers, it
# notices nothing: it goes on from numbers the database no longer has, and
# what the restore lost stays lost.
#
# So each restore needs an epoch no device has seen. A number one higher
# than the restored one will not do:
#
# - An account reset after the dump raised its own epoch by one then.
# - The same dump restored twice would get the same epoch twice, and devices
#   that synced in between would not notice the second restore.
#
# The epoch is therefore set from the clock: the seconds since the start of
# 2026, and at least a thousand more than it was. Two restores a second
# apart get different epochs. It fits the epoch's 32 bits until 2094.
#
# psql must be at least as new as the server (Postgres 18). PSQL names the
# program to run when it is not on the PATH.
set -euo pipefail

: "${RESTORE_DATABASE_URL:?set RESTORE_DATABASE_URL to the database that was restored}"
read -r -a psql <<< "${PSQL:-psql}"

"${psql[@]}" --no-psqlrc --quiet --tuples-only --no-align \
  --set ON_ERROR_STOP=1 --dbname="$RESTORE_DATABASE_URL" --command "
    WITH raised AS (
      UPDATE accounts
         SET epoch = GREATEST(
               epoch + 1000,
               (extract(epoch FROM now())::bigint - 1767225600)::integer)
      RETURNING epoch
    )
    SELECT 'raised the epoch of ' || count(*) || ' accounts'
           || COALESCE(' to ' || max(epoch), '') FROM raised"
