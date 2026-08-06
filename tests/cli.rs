use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn status_json_is_accepted() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .args(["status", "--json"])
        .assert()
        .code(4)
        .stdout(predicate::str::contains("\"ok\":false"));
}

#[test]
fn invalid_command_uses_usage_exit_code() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .arg("unknown")
        .assert()
        .code(2);
}
