-- Step 6 of the redesign: more devices.
-- Design: docs/plans/2026-11-10-system-redesign-design.md, sections 5.4, 5.5
-- and 7.3. Plan: docs/plans/2026-11-12-step-6-more-devices-plan.md.

-- The approval exchange between a waiting device and an approved one. The
-- server relays these messages and reads none of them. They live only while
-- the exchange does.
CREATE TABLE enrolment_messages (
  device_id   uuid NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
  -- Numbers only rise for a device, also across a restart of its exchange,
  -- so a reader's "after N" never misses or repeats a message.
  seq         bigint NOT NULL,
  sender_id   uuid NOT NULL,
  body        bytea NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (device_id, seq)
);

-- The account key, encrypted so that only the waiting device can open it,
-- from the moment an approved device uploads it until the waiting device
-- says it has it.
CREATE TABLE enrolment_keys (
  device_id    uuid PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
  approver_id  uuid NOT NULL,
  key_version  integer NOT NULL,
  wrapped      bytea NOT NULL,
  created_at   timestamptz NOT NULL DEFAULT now()
);

-- The optional recovery copy: the account key encrypted under a key derived
-- from the person's recovery phrase, and the hash of a claim code derived
-- from the same phrase. Neither lets the server open the key.
CREATE TABLE recovery_keys (
  account_id   uuid PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  key_version  integer NOT NULL,
  wrapped      bytea NOT NULL,
  claim_hash   bytea NOT NULL,
  created_at   timestamptz NOT NULL DEFAULT now()
);

-- A reset asked for by a device that is not approved waits, so that the
-- account's working devices can refuse it.
ALTER TABLE accounts
  ADD COLUMN reset_requested_at  timestamptz,
  ADD COLUMN reset_requested_by  uuid;

-- The highest message number ever used for a device's exchange.
ALTER TABLE devices
  ADD COLUMN enrolment_seq  bigint NOT NULL DEFAULT 0;
