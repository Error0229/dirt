# Trial 1: yrs document size and the deletion rule

- **Date:** 2026-11-10
- **Verdict:** pass
- **Code:** `trials/yrs-size/` on the `trials/step-1` branch
- **Plan:** `docs/plans/2026-11-10-refactor-system-redesign-implementation-plan.md`, Step 1
- **Tracking issue:** #237

## Question

The redesign stores each note as one CRDT document and uploads the whole
document on every change. Two things had to be shown before building on that:

1. The document stays small enough to re-upload after many edits.
2. The deletion rule can be implemented correctly: a delete covers only what
   the deleting device had seen.

## Verdict

Both hold.

| Criterion | Threshold | Measured |
|---|---|---|
| Size after 1,000 one-word edits | at most 32 KB | 15.3 KB |
| Growth per edit | at most 30 bytes | 15 bytes |
| Merge, convergence, deletion and client-ID tests | all pass | 31 of 31 pass |

yrs stays the CRDT library (decision 13). The fallback comparison with Loro and
Automerge was not needed and was not run.

## Method

- **Versions:** yrs 0.28.1, similar 2, rustc 1.99.1, release build.
- **Document layout:** the note text, a map of edit counts and edit times per
  device, and a map of deletion markers per device.
- **An edit:** one save. The new text is diffed against the old, and only the
  changed spans are written, in one transaction that also records the edit.
- **Client IDs:** real-sized values just under 2^53, not small test numbers.
  This matters for the v1 encoding, as the results show.
- **Garbage collection:** on, which is the yrs default.
- **Encodings:** both yrs encodings were measured. "v2" is the compact one.

## Size results

Output of `cargo run --release` in `trials/yrs-size/`:

| Scenario | Edits | Text bytes | v1 bytes | v2 bytes | v2 bytes per edit |
|---|---|---|---|---|---|
| Note as first saved (no edits) | 0 | 178 | 230 | 243 | 0.0 |
| Type the note, one character per save | 178 | 178 | 4,831 | 1,331 | 7.4 |
| Change one word, 100 times | 100 | 212 | 6,768 | 2,215 | 19.7 |
| Change one word, 1,000 times | 1,000 | 237 | 54,699 | 15,291 | 15.0 |
| Change one word, 10,000 times | 10,000 | 224 | 546,288 | 148,789 | 14.9 |
| Short note, change one word, 1,000 times | 1,000 | 88 | 50,680 | 13,700 | 13.6 |
| Non-ASCII note, change a few characters | 1,000 | 549 | 41,717 | 12,137 | 11.9 |
| Whole-text replacement, 1,000 times | 1,000 | 237 | 27,155 | 10,327 | 10.1 |
| CLI appends a line, 1,000 runs, same ID | 1,000 | 8,893 | 35,915 | 16,987 | 16.9 |
| CLI appends a line, 1,000 runs, new ID each run | 1,000 | 8,893 | 62,838 | 54,868 | 54.8 |

A deletion marker adds 48 bytes to a 243-byte document.

## Findings

### Size

- **A heavily edited note is still small.** One thousand edits cost about
  15 KB, whatever the note's length or language.
- **Growth is linear and never reverses.** Ten thousand edits cost 149 KB.
  Garbage collection drops deleted text but keeps a small record per edit.
- **Ordinary typing is cheap.** Writing a 178-character note with a save after
  every character ends at 1.3 KB.
- **Use the v2 encoding.** With real client IDs, v1 is 3.6 times larger. The
  earlier research figure of 22 KB for v1 used small test IDs and understated
  it.
- **Keep one client ID per install.** A new ID on every run makes the document
  3.2 times larger for the same edits.
- **Whole-text replacement is smaller but wrong.** It costs 10 KB instead of
  15 KB, and it duplicates concurrent edits, as the merge test shows.

### Merging

- **Edits to different words both survive, once.** Covered for plain and
  non-ASCII text.
- **One save that changes two places still merges with an edit in between.**
  This is why the diff is a real character diff and not a common-prefix and
  common-suffix trim.
- **Edits to the same word converge.** Both devices end with the same text, and
  nothing either typed is lost.
- **Merging is order-independent and repeatable.** The same states merged in
  either order, with repeats, give byte-identical documents.
- **Random histories converge.** Three devices ran 300 histories of 60 steps
  and 10 of 600 steps, mixing edits, deletes, restores and partial syncs. For
  every history, all six merge orders gave the same text, deletion state and
  version. 79 of the 300 histories ended deleted, so both outcomes are covered.

### Deletion rule

The rule works, with one change to the mechanism the design described.

- **What the design said:** a deletion marker holds "the document state" the
  deleter had observed.
- **What does not work:** comparing yrs state vectors. A state vector does not
  move when an edit only removes characters, so such an edit would count as
  already seen. `the_yrs_state_vector_does_not_move_on_a_delete_only_edit`
  demonstrates it. Writing a marker also moves the deleter's own entry.
- **What the trial uses instead:** the document keeps its own count of edits
  per device. Every edit raises the editing device's count. A marker is a copy
  of those counts. The note is deleted while at least one marker covers every
  count.
- **Cost:** the count is one small map write per edit, already included in the
  sizes above. It shares its entry with the edit time, so there is one write,
  not two.
- **A side benefit:** the counts do not depend on yrs internals, so the rule
  would survive a change of CRDT library.

Cases covered by `tests/deletion.rs`:

| Case | Result |
|---|---|
| Delete on A, unseen edit on B | Note comes back with B's edit |
| Delete on A, unseen edit on B that only removes text | Note comes back |
| Delete on A after seeing B's edit | Stays deleted |
| Delete on both | Stays deleted |
| Edit then delete on A, delete of the older state on B | Stays deleted |
| Delete, then restore | Note comes back everywhere |
| Restore, then delete again | Deleted |
| A third device deletes a note that came back | Deleted |

### Client IDs

- **Sharing one ID between the desktop app and the CLI is safe,** provided each
  edit loads the stored document, changes it and stores it again. Fifty
  alternating edits from two simulated processes all survived.
- **A device that lost state can tell.** It compares the incoming document's
  record of its own writes with its own. The check is one function.
- **The hazard is real.** A device that writes under its old ID after losing
  state has its new edit silently dropped by other copies.
  `writing_under_the_old_id_after_losing_state_loses_the_new_edit` shows it.
- **Taking a new ID first avoids it.** The same scenario with a fresh ID keeps
  the edit.

## Limits of this trial

- **Encryption overhead was not measured.** It is a fixed cost per blob of a
  nonce and an authentication tag, about 40 bytes.
- **Real save frequency is unknown.** The measurements count saves. How often
  the editors save decides how fast a real note reaches a given count.
- **Diff speed on very long notes was not measured.** All ten scenarios,
  including 10,000 edits, ran in under eight seconds in total.
- **The diff works on characters, not on what a reader sees as one symbol.** An
  emoji built from several code points could be split by two concurrent edits
  to the same symbol. The result is still valid text.
- **Android was not run.** yrs has no native code, and research found no
  Android-specific code in it, but the trial ran on Windows only.

## Effect on the design

- **Decision 13 stands:** yrs.
- **Section 6.2:** the deletion marker holds edit counts per device kept in the
  document, not the yrs state vector.
- **Section 6.3:** documents are stored and uploaded in the yrs v2 encoding.
- **New dependency:** the `similar` crate, for the text diff.
- **Per-note size cap:** at 15 bytes per edit, a cap of 256 KB allows about
  16,000 edits of a short note. A note that reaches the cap needs a defined
  behaviour. That question joins the "problem with one note" class and should
  be settled in step 3.

## Reproduce

```bash
cd trials/yrs-size
cargo test             # 31 tests
cargo run --release    # the size table
```
