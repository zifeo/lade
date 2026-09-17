use assert_cmd::Command;

#[test]
fn lade_age_plugin_flag_is_unknown() {
    Command::cargo_bin("lade")
        .unwrap()
        .arg("--age-plugin=no-such-v1")
        .assert()
        .failure();
}

#[test]
fn lade_without_plugin_flag_is_still_lade() {
    Command::cargo_bin("lade")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("lade"));
}
