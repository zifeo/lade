use super::lock::{self, LockSlot};
use super::plane::{Plane, Snapshot};
use super::spec::{self, Spec};
use super::toml_merge::{self, DeclaredTool};

#[derive(Clone, Debug)]
pub(super) enum Choice {
    Pin(Spec),
    Row(Spec),
    Builtin(Spec),
}

impl Choice {
    pub(super) fn spec(&self) -> &Spec {
        match self {
            Self::Pin(spec) | Self::Row(spec) | Self::Builtin(spec) => spec,
        }
    }
}

pub(super) fn decide(snap: &Snapshot, key: &str, spec: Spec, yaml: bool) -> Choice {
    if yaml {
        return Choice::Pin(spec);
    }
    if let Some(row) = fitting_row(snap, key, &spec) {
        let mut spec = spec::at_version(&spec, &row.version);
        spec.options = row.options;
        spec.install_id = Some(row.key);
        return Choice::Row(spec);
    }
    Choice::Builtin(spec)
}

pub(super) fn with_spec(choice: Choice, spec: Spec) -> Choice {
    match choice {
        Choice::Pin(_) => Choice::Pin(spec),
        Choice::Row(_) => Choice::Row(spec),
        Choice::Builtin(_) => Choice::Builtin(spec),
    }
}

fn fitting_row(snap: &Snapshot, key: &str, spec: &Spec) -> Option<DeclaredTool> {
    let Plane::Mise {
        toml, toml_write, ..
    } = &snap.plane
    else {
        return None;
    };
    let path = toml.as_ref().unwrap_or(toml_write);
    if !path.is_file() {
        return None;
    }
    toml_merge::declared_candidates(path, key, &spec.backend_id())
        .into_iter()
        .find(|declared| {
            let probe = LockSlot {
                name: declared.key.clone(),
                version: declared.version.clone(),
                backend: Some(spec.backend_id()),
            };
            lock::agrees(&probe, &spec.version, &spec.backend_id())
        })
}

#[cfg(test)]
mod tests {
    use super::{Choice, decide};
    use crate::mise::plane;

    fn project(body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("mise.toml"), body).unwrap();
        dir
    }

    fn implied_infisical() -> crate::mise::spec::Spec {
        crate::mise::spec::parse(&crate::mise::implied::by_key("infisical").unwrap().uri).unwrap()
    }

    fn choose(dir: &std::path::Path, spec: crate::mise::spec::Spec, yaml: bool) -> Choice {
        decide(&plane::scan(dir), "infisical", spec, yaml)
    }

    #[test]
    fn yaml_pin_is_pin() {
        let dir = project("[tools]\ninfisical = \"0.43.55\"\n");
        let spec =
            crate::mise::spec::parse("mise://github/Infisical/cli[asset_pattern=from-yaml]@9.9.9")
                .unwrap();
        let Choice::Pin(chosen) = choose(dir.path(), spec, true) else {
            panic!("expected pin");
        };
        assert_eq!(chosen.version, "9.9.9");
        assert_eq!(
            chosen.options.get("asset_pattern").map(String::as_str),
            Some("from-yaml")
        );
        assert!(chosen.install_id.is_none());
    }

    #[test]
    fn two_part_version_is_row() {
        let dir = project("[tools]\nkubectl = \"1.31\"\n");
        let spec = crate::mise::spec::parse(&crate::mise::implied::by_key("kubectl").unwrap().uri)
            .unwrap();
        let Choice::Row(chosen) = decide(&plane::scan(dir.path()), "kubectl", spec, false) else {
            panic!("expected row");
        };
        assert_eq!(chosen.version, "1.31");
        assert_eq!(chosen.install_id.as_deref(), Some("kubectl"));
    }

    #[test]
    fn in_range_alias_is_row() {
        let dir = project("[tools]\ninfisical = \"0.43.55\"\nnode = \"24\"\n");
        let Choice::Row(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected row");
        };
        assert_eq!(chosen.version, "0.43.55");
        assert_eq!(chosen.install_id.as_deref(), Some("infisical"));
        assert!(chosen.options.is_empty(), "{:?}", chosen.options);
        assert_eq!(chosen.tool_id(), "infisical");
    }

    #[test]
    fn in_range_backend_is_row() {
        let dir = project("[tools]\n\"github:Infisical/cli\" = \"0.43.55\"\n");
        let Choice::Row(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected row");
        };
        assert_eq!(chosen.install_id.as_deref(), Some("github:Infisical/cli"));
        assert!(chosen.options.is_empty(), "{:?}", chosen.options);
    }

    #[test]
    fn row_keeps_declared_options() {
        let dir = project(
            "[tools]\n\"github:Infisical/cli\" = { version = \"0.43.55\", asset_pattern = \"from-toml\" }\n",
        );
        let Choice::Row(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected row");
        };
        assert_eq!(
            chosen.options.get("asset_pattern").map(String::as_str),
            Some("from-toml")
        );
    }

    #[test]
    fn out_of_range_alias_does_not_hide_an_in_range_row() {
        let dir =
            project("[tools]\ninfisical = \"0.1.0\"\n\"github:Infisical/cli\" = \"0.43.55\"\n");
        let Choice::Row(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected row");
        };
        assert_eq!(chosen.version, "0.43.55");
        assert_eq!(chosen.install_id.as_deref(), Some("github:Infisical/cli"));
    }

    #[test]
    fn version_outside_the_range_is_builtin() {
        let dir = project("[tools]\ninfisical = \"0.1.0\"\n");
        let original = implied_infisical();
        let Choice::Builtin(chosen) = choose(dir.path(), original.clone(), false) else {
            panic!("expected builtin");
        };
        assert!(chosen.install_id.is_none());
        assert!(chosen.options.contains_key("asset_pattern"));
        assert_eq!(chosen.version, original.version);
    }

    #[test]
    fn missing_tool_is_builtin() {
        let dir = project("[tools]\nnode = \"24\"\n");
        let Choice::Builtin(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected builtin");
        };
        assert!(chosen.install_id.is_none());
        assert!(chosen.options.contains_key("asset_pattern"));
    }

    #[test]
    fn another_cli_package_is_builtin() {
        let dir = project("[tools]\n\"aqua:1password/cli\" = \"2.30.0\"\n");
        let Choice::Builtin(chosen) = choose(dir.path(), implied_infisical(), false) else {
            panic!("expected builtin");
        };
        assert!(chosen.install_id.is_none(), "{:?}", chosen.install_id);
    }

    #[test]
    fn lade_plane_is_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(
            home.path().join("mise.toml"),
            "[tools]\ninfisical = \"9.9.9\"\n",
        )
        .unwrap();
        temp_env::with_var("HOME", Some(home.path()), || {
            let Choice::Builtin(chosen) = choose(dir.path(), implied_infisical(), false) else {
                panic!("expected builtin");
            };
            assert_ne!(chosen.version, "9.9.9");
            assert!(chosen.options.contains_key("asset_pattern"));
        });
    }
}
