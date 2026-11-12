-- Step 9 of the redesign: attachments.
-- Design: docs/plans/2026-11-10-system-redesign-design.md, section 10.
-- Plan: docs/plans/2026-11-12-step-9-attachments-plan.md.

-- One encrypted file in the object storage. The server holds neither its
-- bytes nor its name nor its key: only that it exists and how large it is.
CREATE TABLE files (
  account_id    uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  -- Made by the device. The object is stored as <account_id>/<file_id>.
  file_id       uuid NOT NULL,
  -- The size the device asked to upload, in bytes. It is counted against
  -- the account from the moment a link is issued.
  size          bigint NOT NULL,
  -- True once a device has confirmed the file for a note and the storage
  -- was seen to hold it at that size.
  confirmed     boolean NOT NULL DEFAULT false,
  created_at    timestamptz NOT NULL DEFAULT now(),
  -- When the object is to be deleted. Set when a link is issued, in case
  -- nothing is ever uploaded or confirmed; cleared when a note refers to
  -- the file; set again when the last note stops.
  delete_after  timestamptz,
  PRIMARY KEY (account_id, file_id)
);
CREATE INDEX files_due ON files (delete_after) WHERE delete_after IS NOT NULL;

-- Which notes refer to a file. The server cannot read a note, so devices
-- tell it. A file may have more than one note: a device that keeps a purged
-- note's text as a new note keeps its files, and two devices may each do
-- so. A file is kept for as long as it has a row here.
CREATE TABLE file_notes (
  account_id  uuid NOT NULL,
  file_id     uuid NOT NULL,
  note_id     uuid NOT NULL,
  PRIMARY KEY (account_id, file_id, note_id),
  FOREIGN KEY (account_id, file_id) REFERENCES files ON DELETE CASCADE
);
CREATE INDEX file_notes_by_note ON file_notes (account_id, note_id);

-- Objects whose rows are gone: an account was deleted or reset. The rows
-- go at once and the objects are deleted afterwards, so that an account can
-- be deleted while the storage cannot be reached.
CREATE TABLE orphan_objects (
  object_key  text PRIMARY KEY,
  created_at  timestamptz NOT NULL DEFAULT now()
);

-- The bytes of the account's files, kept here so that a quota check reads
-- one row.
ALTER TABLE accounts ADD COLUMN file_bytes bigint NOT NULL DEFAULT 0;
