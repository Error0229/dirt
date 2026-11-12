-- Step 8 of the redesign: changing the account's key.
-- Design: docs/plans/2026-11-10-system-redesign-design.md, section 5.5.
-- Plan: docs/plans/2026-11-12-step-8-hardening-plan.md.

-- Each later version of the account key, wrapped for one device. Only that
-- device can open its copy, and only with the key the copy replaces, so
-- the server learns nothing from holding them. A device that was offline
-- when the key changed fetches its copies here, and opens them in order.
--
-- Version 1 has no rows: the first device makes it, and every other device
-- is given the key it joins under by the approval exchange.
CREATE TABLE device_keys (
  device_id    uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
  key_version  integer NOT NULL,
  wrapped      bytea NOT NULL,
  created_at   timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (device_id, key_version)
);

-- How many of the account's notes are still stored under an older key than
-- accounts.key_version. Kept here so that a pull reads it with the row it
-- already reads. It is set when the key changes and falls as notes are sent
-- again under the new key. At zero the change is complete.
ALTER TABLE accounts ADD COLUMN old_key_notes bigint NOT NULL DEFAULT 0;
