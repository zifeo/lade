//! Concrete versions for aqua packages mise cannot list.
//!
//! mise lists aqua versions only when the registry row has `repo_owner` and
//! `repo_name`. `aqua:1password/cli` is HTTP and has neither. `mise latest`
//! exits 0 and prints nothing. Setup installs `version`.
//! `vfox:mise-plugins/vfox-1password` can list the same AgileBits builds.
//! The pin stays aqua so an existing lock still installs.
//!
//! Audited against aquaproj/aqua-registry `main` on 2026-09-29. Every other
//! implied aqua pin in that registry has both fields. Doppler, Infisical,
//! Azure CLI, and gcloud are not aqua packages. `github`, `pipx`, `vfox`,
//! `npm`, and `core` do not use this check.

pub struct Known {
    pub backend_id: &'static str,
    pub version: &'static str,
}

const KNOWN: &[Known] = &[Known {
    backend_id: "aqua:1password/cli",
    version: "2.39.0",
}];

pub(super) fn version_for(backend_id: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|row| row.backend_id == backend_id)
        .map(|row| row.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Registry row has `repo_owner` and `repo_name`, so `mise latest` can list.
    const LISTABLE: &[&str] = &[
        "aqua:getsops/sops",
        "aqua:bitwarden/clients",
        "aqua:hashicorp/vault",
        "aqua:aws/aws-cli",
        "aqua:kubernetes/kubectl",
        "aqua:txn2/kubefwd",
        "aqua:gravitational/teleport",
        "aqua:k3d-io/k3d",
        "aqua:fish-shell/fish-shell",
    ];

    fn backend_id(mise: &str) -> String {
        let (backend, package) = mise.split_once('/').unwrap();
        format!("{backend}:{package}")
    }

    #[test]
    fn only_op_is_unlistable() {
        assert_eq!(KNOWN.len(), 1);
        assert_eq!(KNOWN[0].backend_id, "aqua:1password/cli");
        assert!(super::super::spec::version_is_concrete(KNOWN[0].version));
    }

    #[test]
    fn every_aqua_cli_spec_is_classified() {
        for spec in lade_sdk::compat::CLI_SPECS {
            let Some(mise) = spec.mise else {
                continue;
            };
            if !mise.starts_with("aqua/") {
                continue;
            }
            let id = backend_id(mise);
            let known = version_for(&id).is_some();
            let listable = LISTABLE.contains(&id.as_str());
            assert!(
                known ^ listable,
                "{id} ({}) must be the known unlistable pin or a listable one",
                spec.scheme
            );
        }
    }
}
