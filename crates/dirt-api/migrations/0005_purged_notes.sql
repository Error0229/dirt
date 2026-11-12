-- Step 8 of the redesign: purging deleted notes.
-- Plan: docs/plans/2026-11-12-step-8-hardening-plan.md, part 8e.

-- How many of the account's notes have been purged. A purged note keeps its
-- row, with no blob and a new change number, so that every device learns of
-- the purge by pulling. Those rows are small but are never removed, so
-- there is a limit to how many an account may have (docs/LIMITS.md).
--
-- accounts.note_count no longer counts them: purging is what frees a
-- place.
ALTER TABLE accounts ADD COLUMN purged_count bigint NOT NULL DEFAULT 0;
