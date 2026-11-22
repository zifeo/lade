//! The transport a link runs over, written out after `--` by the caller.

use anyhow::{Result, bail};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Ssh,
    Tsh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transport {
    pub kind: Kind,
    /// As written, with the program first.
    pub argv: Vec<String>,
    pub destination: String,
}

const ACCEPTED: &str = "`ssh …` or `tsh ssh …`";

// Flags of OpenSSH that take a value.
const SSH_VALUED: &str = "BbcDEeFIiJLlmOoPpQRSWw";
// What would break the link's pipe: a tty, no remote command, a closed
// stdin, the background, a subsystem or stdio forward in place of the
// command, a control command, or a printed config in place of a session.
// Lade's own `-o` come first and win over the caller's.
// § bhj0hmdfv.a@7n7aa
const SSH_REFUSED: &str = "tNnfsWOG";
// § end bhj0hmdfv.a
const SSH_REFUSED_OPTIONS: &[&str] = &[
    "remotecommand",
    "requesttty",
    "sessiontype",
    "stdinnull",
    "forkafterauthentication",
];

const TSH_VALUED: &[&str] = &[
    "-l",
    "--login",
    "-p",
    "--port",
    "-o",
    "--option",
    "-i",
    "--identity",
    "-J",
    "--jumphost",
    "--proxy",
    "--user",
    "--cluster",
    "--ttl",
    "--auth",
    "--request-reason",
    "--request-id",
    "--mfa-mode",
    "--relay",
];
const TSH_REFUSED: &[&str] = &[
    "-t",
    "--tty",
    "-N",
    "--no-remote-exec",
    "--fork-after-authentication",
];

impl Transport {
    // § bhj0hmdfv@hreax
    pub(crate) fn parse(argv: &[String]) -> Result<Self> {
        match argv {
            [ssh, rest @ ..] if ssh == "ssh" => parse_ssh(rest).map(|destination| Self {
                kind: Kind::Ssh,
                argv: argv.to_vec(),
                destination,
            }),
            [tsh, sub, rest @ ..] if tsh == "tsh" && sub == "ssh" => {
                parse_tsh(rest).map(|destination| Self {
                    kind: Kind::Tsh,
                    argv: argv.to_vec(),
                    destination,
                })
            }
            [] => bail!("lade connect needs a transport after `--`: {ACCEPTED}."),
            // § bhj0hmdfv.b@ahyek
            [program, ..] => bail!(
                "lade connect runs over {ACCEPTED}, not `{program}`: they authenticate the host before the link carries anything."
            ),
        }
    }

    /// The host part of the destination, for messages.
    pub(crate) fn host(&self) -> &str {
        let host = self
            .destination
            .rsplit_once('@')
            .map_or(self.destination.as_str(), |(_, host)| host);
        host.strip_prefix("ssh://").unwrap_or(host)
    }

    /// The transport running `remote` on the other end. `options` go to
    /// `ssh -o`. `tsh` takes none.
    pub(crate) fn command(&self, options: &[String], remote: &str) -> Command {
        let mut command = Command::new(&self.argv[0]);
        match self.kind {
            Kind::Ssh => {
                // A `RemoteCommand` or `RequestTTY` of the caller's ssh
                // config would break the pipe as their flags would.
                command.args(["-T", "-o", "RemoteCommand=none"]);
                for option in options {
                    command.args(["-o", option]);
                }
                command.args(&self.argv[1..]);
            }
            Kind::Tsh => {
                command.args(&self.argv[1..]);
            }
        }
        command.arg(remote);
        command
    }

    /// The caller's command line, as a remote prints it back.
    pub(crate) fn written(&self) -> String {
        self.argv
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn parse_ssh(args: &[String]) -> Result<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            i += 1;
            break;
        }
        let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
            break;
        };
        for (at, flag) in flags.char_indices() {
            if SSH_REFUSED.contains(flag) {
                bail!("`-{flag}` would break the link's pipe: drop it.");
            }
            if SSH_VALUED.contains(flag) {
                let value = match &flags[at + flag.len_utf8()..] {
                    "" => {
                        i += 1;
                        args.get(i)
                            .map(String::as_str)
                            .ok_or_else(|| anyhow::anyhow!("ssh `-{flag}` needs a value"))?
                    }
                    inline => inline,
                };
                if flag == 'o' {
                    refuse_ssh_option(value)?;
                }
                break;
            }
        }
        i += 1;
    }
    match &args[i.min(args.len())..] {
        [] => bail!("lade connect needs an ssh destination after the flags."),
        [destination] => Ok(destination.clone()),
        [_, command @ ..] => bail!(
            "lade connect runs its own command on the remote: drop `{}`.",
            command.join(" ")
        ),
    }
}

fn refuse_ssh_option(option: &str) -> Result<()> {
    let key = option
        .split(['=', ' ', '\t'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if SSH_REFUSED_OPTIONS.contains(&key.as_str()) {
        bail!("`-o {option}` would break the link's pipe: drop it.");
    }
    Ok(())
}

fn parse_tsh(args: &[String]) -> Result<String> {
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        if TSH_REFUSED.contains(&name) {
            bail!("`{name}` would break the link's pipe: drop it.");
        }
        if let Some(option) = tsh_option(arg, args.get(i + 1)) {
            refuse_ssh_option(option)?;
        }
        if arg.starts_with('-') {
            if TSH_VALUED.contains(&arg.as_str()) {
                i += 1;
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }
    match positional.as_slice() {
        [] => bail!("lade connect needs a tsh destination."),
        [destination] => Ok(destination.clone()),
        [_, command @ ..] => bail!(
            "lade connect runs its own command on the remote: drop `{}`.",
            command.join(" ")
        ),
    }
}

/// The value of a tsh `-o`, in any of its spellings.
fn tsh_option<'a>(arg: &'a str, next: Option<&'a String>) -> Option<&'a str> {
    match arg {
        "-o" | "--option" => next.map(String::as_str),
        _ => arg
            .strip_prefix("--option=")
            .or_else(|| arg.strip_prefix("-o").filter(|value| !value.is_empty())),
    }
}

/// Single-quoted for a POSIX shell when it holds more than safe characters.
pub(crate) fn quote(arg: &str) -> String {
    let safe = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@=,+%~".contains(c));
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Transport> {
        Transport::parse(&line.split(' ').map(str::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn ssh_destination_after_valued_flags() {
        let transport = parse("ssh -J bastion -p 2222 -o User=me build-01").unwrap();
        assert_eq!(transport.kind, Kind::Ssh);
        assert_eq!(transport.destination, "build-01");
        assert_eq!(parse("ssh -p2222 me@build-01").unwrap().host(), "build-01");
    }

    #[test]
    fn what_breaks_the_pipe_is_refused() {
        for line in [
            "ssh -t build-01",
            "ssh -vN build-01",
            "ssh -f build-01",
            "ssh -n build-01",
            "ssh -W db:5432 build-01",
            "ssh -O exit build-01",
            "ssh -o RemoteCommand=bash build-01",
            "ssh -oRequestTTY=yes build-01",
            "tsh ssh -t build-02",
            "tsh ssh --no-remote-exec build-02",
            "tsh ssh -o RequestTTY=yes build-02",
            "tsh ssh --option=RemoteCommand=bash build-02",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn the_rest_runs_as_written() {
        for line in [
            "ssh -A build-01",
            "ssh -L 5432:db:5432 build-01",
            "ssh -o ControlMaster=yes -o ForwardX11=yes build-01",
            "tsh ssh -A build-02",
        ] {
            assert!(parse(line).is_ok(), "{line}");
        }
        let args: Vec<String> = parse("ssh -A build-01")
            .unwrap()
            .command(&[], "lade attach")
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-T",
                "-o",
                "RemoteCommand=none",
                "-A",
                "build-01",
                "lade attach"
            ]
        );
    }

    #[test]
    fn a_remote_command_is_refused() {
        assert!(parse("ssh build-01 uptime").is_err());
        assert!(parse("tsh ssh me@build-02 uptime").is_err());
    }

    #[test]
    fn tsh_destination_after_its_flags() {
        let transport = parse("tsh ssh --proxy proxy.acme.dev -l me build-02").unwrap();
        assert_eq!(transport.kind, Kind::Tsh);
        assert_eq!(transport.destination, "build-02");
    }

    #[test]
    fn other_transports_name_the_accepted_ones() {
        let error = parse("kubectl exec -it pod -- sh").unwrap_err().to_string();
        assert!(error.contains("ssh"), "{error}");
        assert!(parse("tsh ls").is_err());
    }
}
