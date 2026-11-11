-- The first schema of the redesigned server.
-- Design: docs/plans/2026-11-10-system-redesign-design.md, section 7.3.

CREATE TABLE accounts (
  id               uuid PRIMARY KEY,
  -- Who the account belongs to, as the identity provider names them.
  clerk_issuer     text NOT NULL,
  clerk_user_id    text NOT NULL,
  email            text,
  -- The last change number handed out. It is each note's version and the
  -- pull cursor, so the protocol contains no clocks.
  change_counter   bigint NOT NULL DEFAULT 0,
  -- Raised when the account is reset or restored, telling clients to forget
  -- the change numbers they have seen.
  epoch            integer NOT NULL DEFAULT 1,
  key_version      integer NOT NULL DEFAULT 1,
  -- Set once, by the first device, which creates the account key.
  key_established  boolean NOT NULL DEFAULT false,
  -- Usage, kept here so that a quota check reads one row.
  note_count       bigint NOT NULL DEFAULT 0,
  bytes_used       bigint NOT NULL DEFAULT 0,
  created_at       timestamptz NOT NULL DEFAULT now(),
  UNIQUE (clerk_issuer, clerk_user_id)
);

CREATE TABLE devices (
  id          uuid PRIMARY KEY,
  account_id  uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  name        text NOT NULL,
  public_key  bytea NOT NULL,
  -- SHA-256 of the device token. The token itself is never stored.
  token_hash  bytea NOT NULL UNIQUE,
  status      text NOT NULL CHECK (status IN ('pending', 'approved', 'revoked')),
  created_at  timestamptz NOT NULL DEFAULT now(),
  last_seen   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX devices_by_account ON devices (account_id);

CREATE TABLE notes (
  account_id      uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  note_id         uuid NOT NULL,
  change_number   bigint NOT NULL,
  key_version     integer NOT NULL,
  format_version  integer NOT NULL,
  -- The encrypted document. Null once the note has been purged.
  ciphertext      bytea,
  PRIMARY KEY (account_id, note_id)
);
-- Pull reads "everything in this account after change number N", in order.
CREATE UNIQUE INDEX notes_by_change ON notes (account_id, change_number);

-- Single-use values for the Clerk sign-in. Only the hash is kept.
CREATE TABLE nonces (
  nonce_hash  bytea PRIMARY KEY,
  expires_at  timestamptz NOT NULL
);
CREATE INDEX nonces_by_expiry ON nonces (expires_at);

-- Requests per account per window, for the rate limit. Kept in the database
-- because serverless instances share no memory.
CREATE TABLE request_counts (
  account_id    uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  window_start  timestamptz NOT NULL,
  requests      integer NOT NULL,
  PRIMARY KEY (account_id, window_start)
);
