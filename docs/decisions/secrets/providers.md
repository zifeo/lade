# Secrets and providers

A command needs values it should not hold for longer than it runs: a vault password, a cloud key, a token a shell snippet prints, a field of an encrypted file. A rule in `lade.yaml` binds each name to a value, and Lade turns that value into a secret through a provider: 1Password, Infisical, Doppler, Vault, Passbolt, Bitwarden, AWS, Azure and GCP secret managers, age, SOPS, a file, or a shell. A value no provider claims is raw: text typed in the yaml.

Each step can go wrong in a way the user does not see. A provider waiting on a locked app hung the command forever (CHANGELOG 0.20.0, "Provider timeout"). A CLI that wants to ask for a password or an MFA code stopped on a terminal it did not hold. A shell secret that failed passed its stdout on as the value (CHANGELOG 0.20.0, #230). A command printed its secrets back into a terminal or an agent's context. Masking short raw literals such as API versions broke unrelated output (CHANGELOG, #149). Two rules with two 1Password service accounts fetched every secret with the last one (CHANGELOG 0.20.0, #228). A command line with a token in it would land in the diary.

This note states what a value becomes, how long a provider may take, what the command sees when one fails, and how a provider reaches its product. What the command's output and the diary show of a value is [output](output.md), and how a single URI is resolved for scripts and for age is [eval](eval.md). How long a value stays cached, which CLI version setup pins, and what the wrap does with a failed hydrate are decided where the cache, the packages and the wrap are.

## Which provider takes a value

The scheme of a value names its provider, and a value no provider claims is raw (`README.md`, Secret, the provider table and its Raw row). A known scheme the provider rejects must not quietly become a string: the user wrote `file://` to read a file, and getting the URI itself as a password is a silent wrong value. `!` is the documented way to keep a value raw whatever it looks like.

**§ <a id="q3jn8hjmk">q3jn8hjmk</a> A secret value goes to the provider its scheme names, and a value no provider claims is raw.**

- (a) A value whose scheme a secret provider registers belongs to that provider. When the provider rejects it, the value fails with that provider's error.

- (b) A value that is neither a tunnel nor a package, with no scheme or with a scheme no secret provider registers, such as `https://` or `postgres://`, is raw: its text is its value.

- (c) A value that starts with `!` is raw with that one `!` removed, so `!` forces raw and `!!` keeps a leading `!`.

- (d) Only `lade add` warns about a raw value: once, when the secret it writes is raw.

- (e) A value whose scheme a tunnel provider registers is a tunnel: when its URI does not parse, the command fails with that provider's error.

- (f) A `mise://`, `apm://` or `skills://` value is a package.

"URI parsing is strict for known network schemes; malformed URIs fail rather than falling back to raw values" (`docs/architecture.md`), for the reason (a) gives: a `kubectl://` read as text would hand the command a string where it expects a port (test `test_inject_network_parse_error_is_boxed`).

The package schemes are the README's scheme table: `mise://` is a pin, and `apm://` and `skills://` are setup packages, so a package value goes to setup and the pin, never to a secret provider, [aq3qybz6p.d](../packages/pins.md#aq3qybz6p). Lade also reads the older mise form `<backend>:<package>`, such as `npm:prettier`, as a package; no doc names that form, so it is code, not a promise.

Raw is not a vault secret: it is already visible in the yaml (`README.md`, Secret). It is never masked (see [qq076qjq2](output.md#qq076qjq2)) and never cached. It still lives in the secret family because it becomes an env var.

## A provider answers in bounded time

A provider is logged in or not, so it answers within seconds or something is wrong (`README.md`, Secret: "A locked or logged-out vault fails fast"). 1Password, age and SOPS can wait on Touch ID or a hardware key touch, so they get longer. A timeout that left the CLI running would leave an `op` under an `sh://` waiting on Touch ID after Lade gave up (CHANGELOG 0.20.0, "Provider timeout"). `lade status` lists each provider's bound.

**§ <a id="j5kahfy4q">j5kahfy4q</a> Every secret provider answers within a bound, or the hydrate fails with an error that names it.**

- (a) The bound is 10 seconds.

- (b) 1Password, age and SOPS get 30 seconds.

- (c) Past its bound, a provider's call fails with an error that names the provider and the bound, and asks to check that it is unlocked and logged in.

- (d) The `lade add` lookups of a provider obey the same bound.

- (e) A timeout kills the provider CLI's whole process group, so what the CLI started dies with it.

At a terminal, a provider CLI can ask (`README.md`, Secret: `bw unlock`, an MFA code, `sh://read`). A terminal lets only its foreground process group read, and a CLI in the background stops on `SIGTTIN` the moment it asks. 1Password's `op inject` reads its references on stdin and still asks for the account password on the terminal. Lade runs providers in parallel, so they take the terminal in turn. The CLIs `lade add` runs to list what a provider holds ask the same way: `op item list` asks for the account password. A tunnel cannot ask: it runs in the hub, away from any terminal. An age plugin is not a CLI Lade spawns: the age library inside the age provider starts it, so an `age://?plugin=` identity that asks for a PIN fails with the plugin's error.

**§ <a id="j9zpdd26a">j9zpdd26a</a> At a terminal, a provider CLI Lade spawns gets the keyboard while it runs, so it can ask.**

- (a) When Lade holds the terminal in the foreground, the process group of a provider CLI Lade spawns, one fed on stdin and a `lade add` lookup included, gets the terminal while it runs, and Lade takes it back when the CLI ends.

- (b) Provider CLIs that run at the same time get the terminal one after the other.

- (c) Lade holds back its progress frames while a provider holds the terminal.

A CLI that asks needs the terminal to itself: `lade-sdk` exports `terminal_lent` so the program around it can hold its own drawing too (CHANGELOG 0.20.0, lade-sdk).

## All or nothing

A command that runs with half its secrets fails somewhere less obvious, or reaches the wrong target (`README.md`, Tunnel: "it would run without what it needs"). A hydrate either returns every value or fails, and the caller decides what the command does then. A shell secret is its stdout, so an error must never quote stdout. An error lands on stderr, which a terminal shows, a CI log keeps and an agent reads, so no error quotes a resolved value either: two sources that set one name to different values are named, not their values.

**§ <a id="d5fjsqxna">d5fjsqxna</a> A hydrate returns every value or none.**

- (a) One provider's failure fails the whole hydrate.

- (b) A value missing from a provider's answer fails the hydrate.

- (c) A shell secret whose command exits with a failure is an error built from its exit status and its stderr.

- (d) When a secret binding, a tunnel or a mise pin set one variable to different values, the command fails with an error that names only the variable and its two sources.

`PATH` is no exception: a pin's `PATH` that replaced a binding named `PATH` would drop the binding's value with no word.

## 1Password without a person

CI that reads 1Password has no person to unlock an app, so it uses a 1Password service account (`README.md`, More). The token is itself a secret and comes from any provider. A token is scoped to the rule that names it: a repository can hold rules for several 1Password accounts, and a secret fetched with the wrong token fails or reads the wrong vault (CHANGELOG 0.20.0, "1Password service account").

**§ <a id="zk7sjvann">zk7sjvann</a> `1password_service_account` on a rule's `.` gives 1Password a service account token for that rule's secrets.**

- (a) Its value is a secret URI, or a per-user map of them, resolved before the rule's secrets and handed to 1Password as `OP_SERVICE_ACCOUNT_TOKEN`.

- (b) Each secret uses the service account of its own rule, in a wrap, in pre-exec and in pre-tool alike.

## How a provider reaches its product

A provider either drives its product's CLI or resolves in-process (`README.md`, Commands: `lade status` reports `sdk` vs `cli` transport). An in-process provider needs no CLI to hydrate: Vault reads KV v2 over HTTP, and its CLI is pinned only for `vault login` (CHANGELOG, "Providers"; `README.md`, Vault row). Which CLI and version setup pins for a URI is decided with the packages. A CLI that is missing is the user's first problem, so the error says what to install.

**§ <a id="cbck003hy">cbck003hy</a> Each secret provider reaches its product through the product's CLI or in-process, and says which.**

- (a) A provider that resolves in-process reports `sdk`, and one that runs its product's CLI reports `cli`. Vault resolves in-process.

- (b) A provider CLI that is not installed fails with an error that names the provider and where to install it.
