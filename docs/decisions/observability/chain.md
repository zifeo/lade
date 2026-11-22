# The chain

A diary any editor can rewrite without a trace is a story, not a record, and a diary that hides a tampered row hides the evidence. This note binds how each row is sealed to the one before it, and how a broken chain is reported.

## The chain

"Each row is HMAC-SHA256 chained to the previous one. ... A raw `sqlite3` edit of a past row breaks `lade log verify`. Parallel writers take an Immediate transaction, so they append one at a time. ... The chain is tamper-evident, not tamper-proof: anyone who runs this binary can reseal" (`docs/observability.md`, Store; CHANGELOG 0.19.0, Diary chain). The key is a constant of the binary, the same for every version, and each seal also binds the version that wrote it, so a newer Lade still verifies an older row (`src/event/chain/mod.rs`; `a_seal_binds_its_version`): an upgrade must not make an untouched diary, or a pack an older Lade shared, read as tampered. Deleting rows is reserved to `lade log prune`, "the only diary delete" (`docs/observability.md`, Read): a database trigger refuses any other `DELETE` (`delete_trigger_blocks_raw_sql`), and prune "starts a new chain epoch over what remains".

What the chain cannot show: the head row is stored unsealed, so cutting the newest rows and pointing the head at the new last row passes verify; and anyone with the binary reseals. The promise is about a past row edited, inserted or removed between others.

A diary written before 0.19.0 has no chain. Lade seals such rows once, when it adds the chain (`open_seals_rows_an_older_lade_left_unsealed`). Sealing an unsealed row in a diary that already has a chain would seal whatever a raw `sqlite3` insert put there, and an editor without the key could launder any edit by nulling the seals; such a row is a break.

**§ <a id="eqyjmph9x">eqyjmph9x</a> Each diary row is sealed to the row before it, so a raw edit, insert or removal of a past row breaks `lade log verify`.**

- (a) A row's seal is an HMAC-SHA256, under a key compiled into Lade, over the seal of the row before it, the Lade version that wrote it and every stored field of the row.

- (b) Lade appends a row in a transaction that holds the diary's write lock, with the sequence number after the head's and the head's seal as its link.

- (c) The diary refuses a delete, except the one `lade log prune` runs.

- (d) After a prune, the rows that remain are resealed oldest first as a new chain, under the next epoch.

- (e) Lade seals rows that have no seal only when it adds the chain to a diary that had none; in a chained diary, such a row is a break.

- (f) A row or a pack an older Lade sealed verifies under a newer one, each row against the version its seal names.

## A broken chain is shown, not hidden

A tampered diary is still evidence, so a break never hides it: "`lade log` and `lade usage` still print, and say how many rows verify before and after the bad row. `lade log verify` exits non-zero" (CHANGELOG 0.19.0, Diary chain; `docs/observability.md`, Read; `tests/log_source.rs`, `log_and_usage_warn_and_still_print_a_tampered_row`, `log_verify_still_fails_when_tampered`). The check reads the rows and the head in one snapshot, after `lade log` reported a tampered head while a command wrote during the check (CHANGELOG 0.20.0, False tampered diary; `verify_during_appends_never_reports_a_tampered_head`). `lade log share` and `lade log prune` reseal what they read, so they warn like the other readers, rather than turn a broken diary into a clean chain without a word. A pack with no chain cannot be checked, which is reported as a break.

**§ <a id="bqy4mdd5m">bqy4mdd5m</a> A broken chain is reported every time Lade reads the diary or a pack, and Lade still reads every row.**

- (a) `lade log`, `lade usage`, `lade log share` and `lade log prune` check the chain of what they read first, warn on stderr when it is broken, and still go on.

- (b) `lade log verify` exits 1 when any diary or pack it checks is broken.

- (c) Each break names the row, by its sequence number, time and command where it has them, and how many rows verify before and after it.

- (d) A pack with no chain is reported as broken.
