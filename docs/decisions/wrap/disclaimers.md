# Disclaimers

A `disclaimer` withholds a rule's access until a person approves it, and an agent must not be able to approve for one. This note binds who lifts a disclaimer, what an approval code covers, and what `lade approve` runs.

## Disclaimers

A `disclaimer` withholds access until a person reviews it (`README.md`, Human approval). A prompt needs a person who can answer it on stdin: "Interactive prompts are forbidden in hook mode due to shell limitations (stdin hijacking, lack of echo)" (`docs/architecture.md`, Disclaimer Flow). Interactive mode, a human's `lade --` or `lade approve` with stdin and stderr at a terminal, is decided once with the audience, [kgqztjzym.c](../harness/audience.md#kgqztjzym). Everyone else gets the disclaimer, no access, and exit 3, "`DISCLAIMER_WITHHELD` (direct inject/approve fail-closed)" (`docs/architecture.md`, Exit codes). The gate stays in the wrap and `lade set`; the pre-tool hook does not prompt (`docs/protocol.md`, Disclaimer). A human shown a withheld disclaimer gets the code to approve it; an agent never does, [b5862tcfn](../promises.md#b5862tcfn).

**§ <a id="betd87z6r">betd87z6r</a> A disclaimer is lifted only by a person typing `yes` in interactive mode or by a valid approval code; otherwise Lade withholds the access.**

- (a) In interactive mode with no valid approval code, Lade shows the disclaimer and runs the command only after `yes`; any other answer cancels it with nothing injected.

- (b) Outside interactive mode, a wrap shows the disclaimer once and exits with the withheld exit code without running the command.

- (c) A human outside interactive mode is shown the approval code both as a `LADE_APPROVE=<code>` prefix and as `lade approve <code>`.

- (d) Lade settles a command's disclaimer before it calls any provider or the hub for that command's secrets or tunnels.

"A withheld or `pending` disclaimer never `Put`s. Consent first" (`docs/cache.md`, Who talks to it): access withheld until a human approves means no vault was asked and no value was cached for that command either. A wrap, `lade set` and `lade mcp` each settle the disclaimer before they acquire.

In pre-exec, `lade set` exits 3 on a withheld disclaimer too, and cancels the line, [t7kcndtra.c](../promises.md#t7kcndtra). A person who answers anything but `yes` refused the command, and Lade exits 1 as for any refusal (test `a_disclaimer_asks_for_yes`): 3 is the code of a disclaimer no one could answer.

The approve code "is not the T id. ... The id is already on `lade --pretool=x7Km`. Using it as `LADE_APPROVE` would let an agent copy the wrap line and skip the disclaimer" (`docs/protocol.md`, Disclaimer). It is "intentionally not a secret ... its purpose is to break the scriptable `LADE_APPROVE=1` reflex and force a deliberate, fresh copy per command. There is no blanket bypass" (`docs/architecture.md`, Disclaimer Flow). The protocol fixes it as the first 5 hex digits of a SHA-256 of the command and a 5-minute window, and accepts "current and previous windows", so a code copied near a boundary still works.

**§ <a id="zgydw7q2s">zgydw7q2s</a> An approval code approves one command, in the window it was shown and the next one only.**

- (a) The code is derived from the command and the current time window alone.

- (b) A code is checked against the codes of the current window and of the one before, those two alone.

- (c) **Retired.**

No fixed value or variable approves every command: outside interactive mode only a valid code lifts a disclaimer, [betd87z6r](disclaimers.md#betd87z6r). Five hex digits can match by chance, so an old code may equal a current one; the check itself reads two windows and no more.

`lade approve` "reads `LADE_T`, requires `pending: true`, verifies the code, then injects. A successful set cannot be re-approved" (`docs/protocol.md`, Disclaimer). The explicit code is the consent, "so it proceeds without further prompting" (`docs/architecture.md`, Disclaimer Flow, step 7). The ticket was written for one directory; run from another, the same line would match other rules and reach other files. The ticket holds the directory whose yaml the line read, after its leading `cd <dir> &&` segments, so approve compares it with the directory the same segments resolve to from where it runs.

**§ <a id="d17dh2dey">d17dh2dey</a> `lade approve <code>` runs only the command a pre-exec disclaimer withheld, from the directory it was withheld in, as `lade --` would run it.**

- (a) With no pending ticket in `LADE_T`, or a ticket whose disclaimer is not pending, `lade approve` exits 1 and runs nothing.

- (b) When the withheld line, resolved from the directory `lade approve` runs in, reads another directory's yaml than the ticket's, `lade approve` exits 1 and runs nothing.

- (c) A wrong or expired code makes `lade approve` exit 1 and run nothing.

- (d) An approved command runs through the wrap with masking on and without a second question.

- (e) **Retired.**

A command `lade set` prepared in full leaves its ticket no longer pending, so under (a) it cannot be approved again.
