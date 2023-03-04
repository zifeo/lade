# Lade

Automatically load secrets from your preferred vault as environment variables,
and clear them once your shell command is over.

![Demo](./examples/demo.gif)

Compatible shells: [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), [Zsh](https://zsh.sourceforge.io)

Compatible Vault services: [Infisical](https://infisical.com),
[1Password CLI](https://1password.com/downloads/command-line/),
[Doppler](https://www.doppler.com)

## Getting started

You can download the binary executable from
[releases page](https://github.com/zifeo/lade/releases/) on Github, make it
executable and add it to your `$PATH` or use
[eget](https://github.com/zyedidia/eget) to automate those steps.

```
eget zifeo/lade --to $HOME/.local/bin

# via cargo
cargo install lade --locked
cargo install --git https://github.com/zifeo/lade --locked

# upgrade
lade self upgrade
```

## Usage

Lade will run before and after any command you run in your shell. On each run,
it will recursively look for `lade.yml` files in the current directory and its
parents. It will then load any secrets matching the command you are running
using a regex.

```
eval "$(lade on)"

cd examples/terraform
terraform apply
# example = "hello world"

eval "$(lade off)"
```

Note: most of the vault loaders use the corresponding native CLI to operate.
This means you must have them installed locally and your login/credentials must
be valid. Lade may evolve by integrating directly with the corresponding API and
is left as future work.

See [lade.yml](lade.yml) or the [examples](./examples) folders for other uses
cases.

### Infisical loader

```yaml
command regex:
    EXPORTED_ENV_VAR: infisical://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME
```

Note: the `/api` is automatically added to the DOMAIN. This source currently
only support a single domain (you cannot be logged into multiple ones).

### 1Password loader

```yaml
command regex:
    EXPORTED_ENV_VAR: op://VAULT_NAME/SECRET_NAME/FIELD_NAME
```

### Doppler loader

```yaml
command regex:
    EXPORTED_ENV_VAR: doppler://api.doppler.com/PROJECT_NAME/ENV_NAME/SECRET_NAME
```

### Raw loader

```yaml
command regex:
    EXPORTED_ENV_VAR: "value"
```

## Development

```
eval "$(cargo run -- on)"
echo a $A1 $A2 $B1 $B2 $B3 $C1 $C2 $C3
cargo run -- -vvv set echo a
eval "$(cargo run -- off)"
```
