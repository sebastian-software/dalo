use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_should_list_planned_top_level_commands() {
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("source"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("adopt"))
        .stdout(predicate::str::contains("resolve"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn stubbed_command_should_fail_with_clear_message() {
    let mut command = Command::cargo_bin("skillmgr").expect("binary should build");

    command
        .arg("status")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "command `status` is not implemented yet",
        ));
}
