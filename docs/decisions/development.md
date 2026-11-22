# Development

Some rules bind the people building Lade, not the product it ships. This note states the one a check enforces, and names the rules that stay guidance until one does.

## How Lade is built

Two rules bind the people building Lade, each paid for by an incident or a cost the project names.

"Tests never write to the developer's real Lade or mise state. ... `env -i` without `HOME` falls back to the real one: the vault test did, rewrote `~/.local/share/mise/shims`, and broke every agent shell" (`AGENTS.md`, Build & test). A test suite that a developer runs dozens of times a day must not be able to break the tool they use to run it.

**§ <a id="ft4f31hbf">ft4f31hbf</a> Tests keep the Lade and mise state they write in temporary directories, apart from the developer's real ones.**

- (a) CI fails a test job that wrote to the real `HOME`'s Lade or mise state, the home mise config and lock `lade setup` can write included.

The carrier is `scripts/home-canary.sh`, which every CI test job runs around its tests, the installer unit test included. A CI workflow is YAML, which Silcrow does not parse, so the jobs that call the canary carry no marker.

How tests comply is test code, not product: every Lade process an integration test starts gets a temporary `HOME` and Lade's state paths under it (`tests/common/mod.rs`), and a unit test that runs with the real `HOME` keeps Lade's state in a temporary directory (`src/test_state.rs`).

0.18.0 moved every HTTP client to rustls and dropped the vendored OpenSSL (`CHANGELOG.md`, 0.18.0, Single rustls stack), so one static binary ships to every target, the musl image included, at one size. That choice lives in `Cargo.toml`, and a test guards the dependency graph (`tests/binary_weight.rs`). It is not a law here: the deciding file is a manifest, with no code to carry it. The crate count and byte budgets in that test are measurements, not promises.

The rule that docs, examples, tests and fixtures use only fake names and paths (`AGENTS.md`, Fake samples stay fake) is not a law here yet: nothing in the tree refuses a real name, so it is guidance until a check decides it.
