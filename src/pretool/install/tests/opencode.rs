use super::super::opencode::{OpenCodeApi, api_from_version};

#[test]
fn api_follows_the_opencode_major() {
    assert_eq!(api_from_version("1.19.0\n").unwrap(), OpenCodeApi::V1);
    assert_eq!(api_from_version("0.15.4").unwrap(), OpenCodeApi::V1);
    assert_eq!(
        api_from_version("opencode v2.0.29\n").unwrap(),
        OpenCodeApi::V2
    );
    assert_eq!(api_from_version("3.1.0").unwrap(), OpenCodeApi::V2);
    assert!(api_from_version("").is_err());
    assert!(api_from_version("opencode dev").is_err());
}

#[test]
fn each_snapshot_is_read_back_as_its_own_api() {
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        assert_eq!(OpenCodeApi::of(api.snapshot()), Some(api));
    }
    assert_eq!(OpenCodeApi::of("export default {}"), None);
}

/// Lade 0.18 to 0.19.2 rewrite any file holding these markers with v1.
#[test]
fn v2_snapshot_has_no_marker_older_lade_rewrites() {
    let v2 = OpenCodeApi::V2.snapshot();
    for marker in ["LadePretool", "lade hook", "[\"hook\"]", "[\"hook\","] {
        assert!(!v2.contains(marker), "v2 plugin contains {marker}");
    }
}

#[test]
fn v1_snapshot_is_the_published_plugin() {
    let v1 = OpenCodeApi::V1.snapshot();
    assert!(v1.contains("export const LadePretool"));
    assert!(v1.contains(r#""tool.execute.before""#));
}
