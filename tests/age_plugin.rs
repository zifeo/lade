use assert_cmd::Command;

#[test]
fn named_binary_prints_plugin_identity() {
    Command::cargo_bin("age-plugin-lade")
        .unwrap()
        .arg("file:///tmp/key.json?query=.key")
        .assert()
        .success()
        .stdout(predicates::str::contains("AGE-PLUGIN-LADE-1"))
        .stdout(predicates::str::contains("age1lade1"));
}

#[test]
fn lade_age_plugin_flag_unknown_machine_fails() {
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
