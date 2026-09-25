use anyhow::{Context, Result, bail};
use std::io::{self, Write};

use crate::args::{AddCommand, RemoveCommand};
use crate::context::InvocationContext;
use crate::family::Family;
use crate::message_box::{MessageBox, Report};

mod cli;
mod package;
mod secret;
mod tunnel;
mod yaml;

pub use yaml::replace_binding_uri;

pub fn run_add(opts: AddCommand, ctx: &InvocationContext) -> Result<()> {
    let tty = ctx.stdin_is_terminal && ctx.stderr_is_terminal;
    let (family, query) = resolve_family(opts.family.as_deref(), opts.query.as_deref(), tty)?;
    let rule = require_or_ask(opts.rule.as_deref(), "Rule (regex): ", tty)?;
    let key = match family {
        Family::Package => opts
            .key
            .clone()
            .or_else(|| package::key_from_query_or_uri(query.as_deref(), opts.uri.as_deref()))
            .or_else(|| {
                tty.then(|| ask("Key (env name, default argv0): ").ok())
                    .flatten()
                    .filter(|s| !s.is_empty())
            }),
        _ => Some(require_or_ask(
            opts.key.as_deref(),
            "Key (env or port): ",
            tty,
        )?),
    };
    let Some(key) = key else {
        bail!("pass --key, or a package name");
    };
    let uri = match opts.uri.clone() {
        Some(uri) => uri,
        None if family == Family::Package => package::package_uri(query.as_deref(), tty)?,
        None if family == Family::Secret && tty => secret::secret_uri()?,
        None if family == Family::Tunnel && tty => tunnel::tunnel_uri()?,
        None if family == Family::Tunnel => {
            require_or_ask(None, "Tunnel URI (kubectl://, tsh://, ssh://): ", tty)?
        }
        None => bail!("pass --uri"),
    };
    if family == Family::Secret && crate::family::is_raw_secret(&uri) {
        warn_raw();
    }
    if Family::of_uri(&uri) != family {
        bail!(
            "URI is a {}, not a {}",
            Family::of_uri(&uri).token(),
            family.token()
        );
    }
    let uri = if family == Family::Package {
        crate::mise::pin_exact(&uri)?
    } else {
        uri
    };
    let path = yaml::target_yaml(tty)?;
    yaml::upsert_binding(&path, &rule, &key, &uri)?;
    Report::new()
        .heading(format!("Wrote {} `{key}` on `{rule}`.", family.token()))
        .line(path.display().to_string())
        .dim("Running `lade setup` for this repo.")
        .print();
    Ok(())
}

pub fn run_remove(opts: RemoveCommand, ctx: &InvocationContext) -> Result<()> {
    let tty = ctx.stdin_is_terminal && ctx.stderr_is_terminal;
    let (family, query) = match (opts.family.as_deref(), opts.query.as_deref()) {
        (Some(first), rest) if let Some(family) = Family::parse(first) => {
            (family, rest.map(str::to_string))
        }
        (Some(first), None) => (Family::Secret, Some(first.to_string())),
        (Some(first), Some(second)) => (Family::Secret, Some(format!("{first} {second}"))),
        (None, _) if tty => {
            let family = ask_family()?;
            (family, opts.query.clone())
        }
        _ => bail!("pass a family (`secret`, `package`, `tunnel`) or a key"),
    };
    let rule = require_or_ask(opts.rule.as_deref(), "Rule (regex): ", tty)?;
    let key = opts
        .key
        .clone()
        .or(query)
        .or_else(|| tty.then(|| ask("Key: ").ok()).flatten());
    let Some(key) = key.filter(|s| !s.is_empty()) else {
        bail!("pass --key");
    };
    let path = yaml::nearest_yaml()?.with_context(|| "no lade.yaml on the walk")?;
    let Some(uri) = yaml::drop_binding(&path, &rule, &key)? else {
        bail!("no `{key}` on `{rule}` in {}", path.display());
    };
    let mut report = Report::new()
        .heading(format!("Removed {} `{key}` on `{rule}`.", family.token()))
        .line(path.display().to_string());
    if uri.contains("teardown=") {
        report = report.dim("Teardown commands run on `lade teardown`.");
    }
    report.print();
    Ok(())
}

fn resolve_family(
    first: Option<&str>,
    second: Option<&str>,
    tty: bool,
) -> Result<(Family, Option<String>)> {
    match first {
        None if tty => Ok((ask_family()?, second.map(str::to_string))),
        None => bail!("pass a family: secret, package, or tunnel"),
        Some(token) if let Some(family) = Family::parse(token) => {
            Ok((family, second.map(str::to_string)))
        }
        Some(query) => Ok((Family::Package, Some(query.to_string()))),
    }
}

fn ask_family() -> Result<Family> {
    let answer = ask("Family (secret, package, tunnel): ")?;
    Family::parse(&answer).with_context(|| format!("unknown family '{answer}'"))
}

pub(super) fn ask(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().to_string())
}

pub(super) fn require_or_ask(value: Option<&str>, prompt: &str, tty: bool) -> Result<String> {
    if let Some(value) = value.filter(|s| !s.is_empty()) {
        return Ok(value.to_string());
    }
    if tty {
        let answer = ask(prompt)?;
        if answer.is_empty() {
            bail!("a value is required");
        }
        return Ok(answer);
    }
    bail!("pass the missing flag, or run in a TTY")
}

pub(super) fn warn_raw() {
    MessageBox::new()
        .warning()
        .line(crate::family::RAW_WARNING)
        .print_stderr();
}
