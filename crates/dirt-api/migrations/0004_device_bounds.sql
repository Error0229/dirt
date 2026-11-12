-- Step 8 of the redesign: the limits review.
-- Plan: docs/plans/2026-11-12-step-8-hardening-plan.md, part 8c.
-- The numbers and their reasons: docs/LIMITS.md.

-- When a device was removed. The server forgets a removed device some time
-- after this, so that an account's rows stay bounded however many devices
-- come and go.
ALTER TABLE devices ADD COLUMN revoked_at timestamptz;

-- Devices removed before this column existed count as removed now.
UPDATE devices SET revoked_at = now() WHERE status = 'revoked';
