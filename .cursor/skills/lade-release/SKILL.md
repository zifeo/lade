---
name: lade-release
description: >-
  Prepare a Lade release: update CHANGELOG.md and bump crate versions with
  cargo set-version. Use when cutting a release, tagging vX.Y.Z, publishing to
  GitHub Releases, or finalizing the changelog before a version bump.
---

# Lade release

## Scope

Release prep touches:

- `CHANGELOG.md` (Keep a Changelog format)
- Crate versions via `cargo set-version --workspace` (`lade`, `lade-sdk`, `age-plugin-lade`, including path deps)
- Crate versions only. There is no APM package.

Never commit, tag, or push — the user handles git.

## Changelog workflow

1. **Find the base tag**
   ```bash
   git tag --sort=-v:refname | head -5
   git log <last-tag>..HEAD --format="%h %s"
   ```

2. **Read `[Unreleased]`** in `CHANGELOG.md`. Every user-facing item there should land in the new version section.

3. **Cross-check commits** since the last tag. Add anything missing from `[Unreleased]` (group under Added / Changed / Fixed / Removed). Skip routine `chore(deps)` unless security-relevant.

4. **Promote `[Unreleased]` → `[X.Y.Z] - YYYY-MM-DD`**
   - Use today's date for stable releases.
   - Leave an empty `## [Unreleased]` section at the top.
   - Add a compare link immediately after the new section:
     ```markdown
     [X.Y.Z]: https://github.com/zifeo/lade/compare/v<prev>...vX.Y.Z
     ```
   - Previous tag is usually the last stable tag on `main` (ignore unreleased beta tags unless the user says otherwise).

5. **Writing style** (match existing entries):
   - Lead with **bold feature name**, then PR link `([#NNN](https://github.com/zifeo/lade/pull/NNN))`.
   - One line per item; explain *why* when behavior changes.
   - Categories: Added, Changed, Fixed, Removed (omit empty categories).

## Version bump

Use `scripts/set-version.sh`, not a bare `cargo set-version` and not a hand edit of `Cargo.toml`:

```bash
bash scripts/set-version.sh X.Y.Z
cargo test --workspace --locked
```

The script runs `cargo set-version --workspace` (`lade`, `lade-sdk`, `age-plugin-lade`, path deps). Run tests (same as CI) to refresh `Cargo.lock` and verify the bump.

If `cargo set-version` is missing: `cargo install cargo-edit`. CI beta bumps use `bash scripts/set-version.sh --bump beta`.

## GitHub release notes

The tag workflow publishes the matching `CHANGELOG.md` section with [parse-changelog](https://github.com/taiki-e/parse-changelog). Write that section before tagging. If the GitHub release already exists, its title and notes stay as first published. A crashed job is recovered by re-running that job. An asset already on the release is left in place.

## Checklist

```
- [ ] `[Unreleased]` promoted to `[X.Y.Z]` with correct date
- [ ] Compare link added (`v<prev>...vX.Y.Z`)
- [ ] All commits since last tag accounted for
- [ ] `bash scripts/set-version.sh X.Y.Z` applied (no `-beta` left unless intentional)
- [ ] GitHub release tag `vX.Y.Z` is the APM pin (`zifeo/lade#vX.Y.Z`)
- [ ] `cargo test --workspace --locked` passes
```
