# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Lade (/leɪd/) is a tool allowing you to automatically load secrets from your
preferred vault into environment variables. It limits the exposure of secrets to
the time the command requiring the secrets lives.

![Demo](./examples/demo.gif)

> Lade is part of the
> [Metatype ecosystem](https://github.com/metatypedev/metatype). Consider
> checking out how this component integrates with the whole ecosystem and browse
> the
> [documentation](https://metatype.dev?utm_source=github&utm_medium=readme&utm_campaign=lade)
> to see more examples.

## Getting started

You can download the binary executable from
[releases page](https://github.com/zifeo/lade/releases/) on GitHub, make it
executable and add it to your `$PATH` or use
[eget](https://github.com/zyedidia/eget) to automate those steps.

```
eget zifeo/lade --to $HOME/.local/bin

# via cargo
cargo install lade --locked
cargo install --git https://github.com/zifeo/lade --locked

# upgrade
lade upgrade

# install/uninstall shell auto launch (see below)
lade install
lade uninstall
```

Compatible shells: [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), [Zsh](https://zsh.sourceforge.io)

Compatible vaults: [Infisical](https://infisical.com),
[1Password CLI](https://1password.com/downloads/command-line/),
[Doppler](https://www.doppler.com), [Vault](https://github.com/hashicorp/vault)

## Usage

Lade will run before and after any command you run in your shell. On each run,
it will recursively look for `lade.yml` files in the current directory and its
parents. It will then aggregate any secrets matching the command you are running
using a regex and load them into environment variables for the time of the run.

```
eval "$(lade on)"

cd examples/terraform
terraform apply
# example = "hello world"

eval "$(lade off)"
```

You can also add `eval "$(lade on)"` to your shell configuration file (e.g.
`~/.bashrc`, `~/.zshrc` or `~/.config/fish/config.fish`) to automatically enable
Lade on each shell session (`lade install` will configure this for you).

Note: most of the vault loaders use their native CLI to operate. This means you
must have them installed locally and your login/credentials must be valid. Lade
may evolve by integrating directly with the corresponding API, but this is left
as future work.

See [lade.yml](lade.yml) or the [examples](./examples) folders for other uses
cases.

### Infisical loader

```yaml
command regex:
    EXPORTED_ENV_VAR: infisical://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME
```

Frequent domain(s): `app.infisical.com`.

Note: the `/api` is automatically added to the DOMAIN. This source currently
only support a single domain (you cannot be logged into multiple ones).

### 1Password loader

```yaml
command regex:
    EXPORTED_ENV_VAR: op://DOMAIN/VAULT_NAME/SECRET_NAME/FIELD_NAME
```

Frequent domain(s): `my.1password.eu`, `my.1password.com` or `my.1password.ca`.

### Doppler loader

```yaml
command regex:
    EXPORTED_ENV_VAR: doppler://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME
```

Frequent domain(s): `api.doppler.com`.

### Vault loader

```yaml
command regex:
    EXPORTED_ENV_VAR: vault://DOMAIN/MOUNT/KEY/FIELD
```

### File loader

Supports INI, JSON, YAML and TOML files.

```yaml
command regex:
    EXPORTED_ENV_VAR: file://PATH?query=.fields[0].field
```

`PATH` can be relative to the lade directory, start with `~`/`$HOME` or absolute
(not recommended when sharing the project with others as they likely have
different paths).

### Raw loader

```yaml
command regex:
    EXPORTED_ENV_VAR: "value"
```

Escaping a value with `!` enforces the use of the raw loader and double `!!`
escapes itself.

## Development

```
eval "$(cargo run -- on)"
echo a $A1 $A2 $B1 $B2 $B3 $C1 $C2 $C3
cargo run -- -vvv set echo a
eval "$(cargo run -- off)"
```
