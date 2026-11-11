-- The real schema in miniature: one counter per account, one row per note.
CREATE TABLE accounts (
  id             uuid PRIMARY KEY,
  change_counter bigint NOT NULL DEFAULT 0
);

CREATE TABLE notes (
  account_id    uuid   NOT NULL REFERENCES accounts(id),
  note_id       uuid   NOT NULL,
  change_number bigint NOT NULL,
  ciphertext    bytea  NOT NULL,
  PRIMARY KEY (account_id, note_id)
);

-- Pull reads "everything in this account after change number N", in order.
CREATE INDEX notes_by_change ON notes (account_id, change_number);
