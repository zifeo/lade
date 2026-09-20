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
