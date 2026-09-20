---
name: finalize
description: >-
  Finalize a merge request: rebase onto main, run bugs, simplify,
  and specs in parallel, apply, then refactor last. Use when the
  user asks to finalize, close a PR, or run the review pageant.
disable-model-invocation: true
---

# Finalize

A pageant, not a swarm. Reviewers are read-only. The parent waits,
edits, then launches the next seat. One pass.

## When

User says `/finalize`, finalize, close the MR, or ready to merge.
Do not run this on a drive-by question.

## 0. Sit on main first

Fetch `origin/main`. Rebase this branch onto it (or merge `main` if
rebase is refused). If the rebase conflicts, stop and ask. Do not
launch reviewers on a stale base.

Then pin the range:

```bash
git fetch origin main
git merge-base origin/main HEAD
git diff --stat "$(git merge-base origin/main HEAD)"
```

If the diff is empty after the rebase, stop.

Build a `Spec:` block for the specs child, in this order:

1. Specs the user stated in this conversation
2. The PR body / issue, if this is an MR
3. A file the user named as the spec

If none of those exist, still launch specs. It must ask. Do not
drop the seat.

```text
Full Repository Path: <abs>
Diff: branch changes
Base Branch: main
Range: <merge-base>...<HEAD>
Spec:
<quoted lines, or "none, ask">
```

Custom agent names may be missing on the Task tool. Default path:
`generalPurpose` children. Paste `.cursor/agents/<name>.md` as the
preamble, then the scope block.

## 1. Parallel: bugs, simplify, specs

Launch these three together. Wait for all three. Do not launch
`refactor` here.

| Subagent | Job | Writes |
| --- | --- | --- |
| `bugs` | Concrete failures | no |
| `simplify` | Smaller form, same behavior | no |
| `specs` | Diff vs a cited spec. Asks if no reference | no |

If `specs` returns `Ask:`, answer from this conversation or ask
the user. Then relaunch **only** `specs` once with the answer.
Do not invent product.

Apply only if useful. Useful means a real failure, or a
call site that should have used an **existing** helper for the
same concern. Not useful: taste, a new generic, or a `wc -l`
move that does not change how a human reads production code.

1. **bugs** critical and high, if the failure is real.
2. **specs** `wrong` / `missing` that cite a reference.
3. **simplify** reuse of a named existing helper, or a
   dead wrapper with one caller. Do not extract a new helper
   as a leftover. Hunt the crate, not only the hunk.

Narrowest check that proves the edit. Stay in the Cursor sandbox
for cargo.

## 2. Last: refactor

Only after step 1 is applied. Launch `refactor` alone.

| Subagent | Job | Writes |
| --- | --- | --- |
| `refactor` | Files over 300 (soft) / 350 (hard), including markdown | no |

Apply a `hard` file only if the split is two jobs a human
can hold (production code). Relocating `#[test]` fns or a
`#[cfg(test)]` block to beat 350 is not useful. Soft (301–350)
is a proposal; apply only if the split is obvious and local.
Keep public API and test names.

Narrowest check again.

## 3. Do not

- Commit, push, merge, or enable auto-merge
- Request `all` for a compile
- Launch refactor next to bugs
- Let specs invent a contract
- Mention source control unless the user asked

## 4. Report

One short status. Then leftovers as a table, only rows where
`Useful?` is `yes`:

| Item | Useful? | Why |
| --- | --- | --- |

`Useful?` is `yes` only for a real failure or an existing
helper to call. Skip taste, new helpers, and `wc -l` moves.
A leftover is only a still-real failure or a named existing
helper the diff still misses. "The reviewer mentioned it" is
not a leftover.
