# Onboarding

How a human or an agent gets Lade wired. Desired state. The binary does all of it.

## Commands

```bash
lade setup
lade teardown
```

`setup` wires this environment. `teardown` removes that wiring. Neither touches the Lade binary.

There is no `lade install` and no `lade uninstall`. The curl installer and `lade upgrade` are how the binary arrives. `lade on` / `lade off` pause pre-exec in this shell. They do not write agent files.

Surgical writes stay on the hook command, always for one agent:

```bash
lade hook enable --harness cursor
lade hook disable --harness cursor
```

`--scope user` exists on `lade hook` only. `setup` never offers it.

## Two lifetimes, one setup

One command. Two blocks in the box. Not two CLIs.

**This repo.** Everyday `lade setup` is this git tree: locks, agent hooks, setup commands.

**This shell.** Once per profile. The first `lade setup` that finds no pre-exec writes `~/.zshrc` (or bash/fish). The box says to reload (`source ~/.zshrc` or a new terminal). Later setups skip that. Missing wrap: warning, `lade hook enable --shell`. `on` / `off` pause it. `lade teardown` does not remove it (`lade hook disable --shell`).

Outside a git repo, setup may still bootstrap this shell, then stop. No `~/.cursor`, no `.cursor` in a scratch folder.

Non-interactive is the same rule. No git: zero agent writes.

## Project, not machine

Agent hooks belong in the repo. Hooks under the home directory (`~/.cursor/hooks.json`, `~/.claude/settings.json`, …) wrap every repo on this machine. That is a leftover, not a path.

`setup` never recommends, defaults to, or writes those home files.

If they already exist, `setup` flags them. It says they wrap every repo, and that this repo should own the hooks instead. It does not add more home hooks. It does not stack a repo hook on top without saying both will run.

If project hooks are already current, `setup` says so and leaves them. Stale Lade hooks in this repo are rewritten. Unrelated hook entries stay.

If user and project hooks both exist for the same agent, that is an error. Tear one plane down before `setup` writes anything else.

## Agents that are actually here

`setup` looks at which agent homes exist on this machine (`.cursor`, `.claude`, Codex home, OpenCode config).

Interactive: it names only those, and asks. One agent installed, one question. None installed: this shell is still wrapped, the box says no agent was found.

Flags (`--cursor`, `--claude`, `--codex`, `--opencode`) skip the question and take that list.

Non-interactive, in a git repo, with no flags: write project hooks for every detected agent. Write nothing for an agent that is not on the machine.

## What the box says

Always a box. One place, one list.

- pre-exec: this shell, this machine, path, current / updated / missing
- pre-tool: this repo `<path>`, or “not a git repo, agents skipped”
- each detected agent: current / updated / missing / flagged (home leftover)

`lade status` uses the same words. On drift it says `run \`lade setup\``.

Lade does not install its own `SKILL.md`. An old official Lade skill file (frontmatter `name: lade`, the phrases Lade used to ship) is deleted if it is still under home or in this repo. The box says the hook already wraps the command. Skills the repo declared with `apm://` or `skills://` are left alone.

## Teardown

`teardown` removes pre-exec from this shell.

In a git repo it removes Lade project hooks for the agents it put there. It does not delete the user’s other hook entries. It reverses each binary `?post_install_cmd=` and asks APM (or the skills CLI) to uninstall declared packages.

Home-directory agent hooks are left alone unless someone is explicit:

```bash
lade hook disable --scope user --harness cursor
```

A folder with no git is not a reason to touch `$HOME`.

## How the binary arrives

Humans:

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
lade setup
```

Agents, from a git repo. Installs the binary if needed, then `lade setup`, never a prompt:

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/agent-setup.sh | bash
```

`setup` also ensures a compatible mise exists (installs it if missing). Lade does not ship an official APM or skill package for itself. A repo that wants hooks or skills declares `apm://` (preferred) or `skills://` in `lade.yml`.

## Out of scope

Lade does not ship a skill that tells agents to call Lade. Agents type the command. The hook rewrites it.

A repo that wants a human sentence for newcomers puts it in its own `AGENTS.md`. Lade does not generate that file.
