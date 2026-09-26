use std::fs;
use std::os::unix::fs::symlink;

use serde_json::Value;

mod common;
use common::dalo_command;

#[test]
fn cold_start_installs_the_embedded_skill_offline_then_delivers_it_through_sync() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("agent-skills");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["init"])
        .assert()
        .success();
    let before_state = fs::read(store.join("state.toml")).unwrap();
    let preview = dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["--json", "--dry-run", "assistant", "install"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let preview: Value = serde_json::from_slice(&preview).unwrap();
    assert_eq!(preview["action"], "install");
    assert_eq!(preview["dry_run"], true);
    assert!(!store.join("local/skills/dalo").exists());
    assert!(!store.join(".assistant-installing").exists());
    assert_eq!(fs::read(store.join("state.toml")).unwrap(), before_state);

    // No Git or downloader can run: the assistant is embedded in the binary.
    let installed = dalo_command()
        .arg("--store")
        .arg(&store)
        .env("PATH", temporary.path().join("no-programs"))
        .args(["--json", "assistant", "install"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let installed: Value = serde_json::from_slice(&installed).unwrap();
    assert_eq!(installed["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(installed["action"], "install");
    assert!(!target.exists());
    assert_eq!(fs::read(store.join("state.toml")).unwrap(), before_state);
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["--dry-run", "sync", "--check"])
        .assert()
        .success();
    assert!(!target.join("dalo").exists());
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["sync", "--check"])
        .assert()
        .success();
    assert!(target.join("dalo").is_symlink());
    for resource in [
        "SKILL.md",
        "agents/openai.yaml",
        "references/setup.md",
        "references/inventory.md",
        "references/migration.md",
        "references/maintenance.md",
    ] {
        assert_eq!(
            fs::read(target.join("dalo").join(resource)).unwrap(),
            fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("skills/dalo")
                    .join(resource)
            )
            .unwrap()
        );
    }
    for command in ["status", "doctor"] {
        dalo_command()
            .arg("--store")
            .arg(&store)
            .args([command, "--json", "--check"])
            .assert()
            .success();
    }
    let repeat = dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["--json", "assistant", "install"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&repeat).unwrap()["action"],
        "existing"
    );
}

#[test]
fn assistant_install_does_not_take_over_a_foreign_target_or_migrate_existing_skills() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("target");
    let external = temporary.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("SKILL.md"), "# Existing assistant\n").unwrap();
    fs::create_dir_all(target.join("my-skill")).unwrap();
    fs::write(target.join("my-skill/SKILL.md"), "# My skill\n").unwrap();
    symlink(&external, target.join("dalo")).unwrap();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["assistant", "install"])
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["sync", "--check"])
        .assert()
        .code(1);
    assert_eq!(fs::read_link(target.join("dalo")).unwrap(), external);
    assert!(!store.join("local/skills/my-skill").exists());
    assert_eq!(
        fs::read_to_string(target.join("my-skill/SKILL.md")).unwrap(),
        "# My skill\n"
    );
}

#[test]
fn missing_and_malformed_stores_are_not_initialized_or_repaired() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["--json", "--dry-run", "assistant", "install"])
        .assert()
        .code(1);
    assert!(!store.exists());
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    fs::write(store.join("config.toml"), "broken = [").unwrap();
    let failure = dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["--json", "assistant", "install"])
        .assert()
        .code(1)
        .get_output()
        .stderr
        .clone();
    assert!(serde_json::from_slice::<Value>(&failure).unwrap()["error"]["message"].is_string());
    assert_eq!(
        fs::read_to_string(store.join("config.toml")).unwrap(),
        "broken = ["
    );
    assert!(!store.join("local/skills/dalo").exists());
}

fn assistant_status(store: &std::path::Path) -> Value {
    let output = dalo_command()
        .arg("--store")
        .arg(store)
        .args(["--json", "assistant", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn make_bundle_stale(store: &std::path::Path) {
    let receipt = store.join("local/skills/dalo/.dalo-bundle.toml");
    let mut data: toml::Value = toml::from_str(&fs::read_to_string(&receipt).unwrap()).unwrap();
    data["dalo_version"] = toml::Value::String("0.0.0".to_owned());
    fs::write(receipt, toml::to_string(&data).unwrap()).unwrap();
}

#[test]
fn status_reports_missing_current_stale_modified_and_delivery_without_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("target");
    assert_eq!(assistant_status(&store)["state"], "missing_store");
    assert!(!store.exists());
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    assert_eq!(assistant_status(&store)["state"], "missing");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["assistant", "install"])
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&target)
        .assert()
        .success();
    let pending = assistant_status(&store);
    assert_eq!(pending["state"], "current");
    assert_eq!(
        pending["undelivered_targets"],
        serde_json::json!(["generic"])
    );
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["sync", "--check"])
        .assert()
        .success();
    let current = assistant_status(&store);
    assert_eq!(current["undelivered_targets"], serde_json::json!([]));
    assert_eq!(current["external_paths"], serde_json::json!([]));
    assert!(current["next_command"].is_null());
    make_bundle_stale(&store);
    let receipt = store.join("local/skills/dalo/.dalo-bundle.toml");
    let before = fs::read(&receipt).unwrap();
    assert_eq!(assistant_status(&store)["state"], "update_available");
    assert_eq!(fs::read(&receipt).unwrap(), before);
    let output = dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["status", "--json", "--check"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["assistant"]["state"],
        "update_available"
    );
    fs::write(store.join("local/skills/dalo/notes.md"), "My notes").unwrap();
    assert_eq!(assistant_status(&store)["state"], "blocked");
    assert_eq!(fs::read(&receipt).unwrap(), before);
}

#[test]
fn status_preserves_external_and_incomplete_installations() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let mut command = dalo_command();
    let foreign = command.test_environment().home.join(".agents/skills/dalo");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("SKILL.md"), "# Installed by another manager").unwrap();
    let output = command
        .arg("--store")
        .arg(&store)
        .args(["assistant", "status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let external: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(external["state"], "external");
    assert_eq!(external["external_paths"], serde_json::json!([foreign]));
    assert!(!store.exists());
    fs::create_dir(&store).unwrap();
    fs::write(store.join("notes.md"), "Preserve partial store").unwrap();
    assert_eq!(assistant_status(&store)["state"], "blocked");
    assert!(!store.join("config.toml").exists());
}

fn in_terminal(
    store: &std::path::Path,
    arguments: &[&str],
    answer: &str,
    environment: &[(&str, &str)],
) -> std::process::Output {
    use std::io::{Read, Write};
    use std::process::Stdio;
    let isolated = dalo_command();
    let executable = assert_cmd::cargo::cargo_bin!("dalo");
    let mut command = std::process::Command::new("/usr/bin/script");
    isolated.test_environment().apply_to(&mut command);
    command.env_remove("CI").env("DALO_ASSISTANT_CHECK", "auto");
    for (key, value) in environment {
        command.env(key, value);
    }
    #[cfg(target_os = "macos")]
    command
        .args(["-q", "/dev/null"])
        .arg(executable)
        .arg("--store")
        .arg(store)
        .args(arguments);
    #[cfg(not(target_os = "macos"))]
    {
        let quote = |value: &std::ffi::OsStr| {
            format!("'{}'", value.to_string_lossy().replace('\'', "'\"'\"'"))
        };
        let mut parts = vec![
            quote(std::path::Path::new(executable).as_os_str()),
            "--store".to_owned(),
            quote(store.as_os_str()),
        ];
        parts.extend(
            arguments
                .iter()
                .map(|argument| quote(std::ffi::OsStr::new(argument))),
        );
        command
            .args(["-q", "-e", "-c"])
            .arg(parts.join(" "))
            .arg("/dev/null");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let answer = answer.to_owned();
    let reader = std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut byte = [0];
        let mut answered = false;
        while output.read(&mut byte).unwrap() > 0 {
            captured.push(byte[0]);
            if !answered && captured.ends_with(b"[y/N] ") {
                input.write_all(answer.as_bytes()).unwrap();
                input.flush().unwrap();
                answered = true;
            }
        }
        captured
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("terminal command timed out: {arguments:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let stdout = reader.join().unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    assert!(status.success(), "{}", String::from_utf8_lossy(&stdout));
    std::process::Output {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn first_interactive_call_waits_for_yes_and_rechecks_on_later_calls() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let decline = in_terminal(&store, &[], "n\n", &[]);
    assert!(String::from_utf8_lossy(&decline.stdout).contains("[y/N]"));
    assert!(!store.exists());
    let accept = in_terminal(&store, &[], "yes\n", &[]);
    assert!(
        String::from_utf8_lossy(&accept.stdout).contains("assistant: installed"),
        "{}",
        String::from_utf8_lossy(&accept.stdout)
    );
    assert_eq!(assistant_status(&store)["state"], "current");
    let repeat = in_terminal(&store, &["next"], "\n", &[]);
    assert!(!String::from_utf8_lossy(&repeat.stdout).contains("[y/N]"));
    make_bundle_stale(&store);
    let receipt = store.join("local/skills/dalo/.dalo-bundle.toml");
    let before = fs::read(&receipt).unwrap();
    let decline = in_terminal(&store, &["next"], "\n", &[]);
    assert!(
        String::from_utf8_lossy(&decline.stdout).contains("Update the bundled Dalo assistant?")
    );
    assert_eq!(fs::read(&receipt).unwrap(), before);
    in_terminal(&store, &["next"], "y\n", &[]);
    assert_eq!(assistant_status(&store)["state"], "current");
}

#[test]
fn initialization_offers_the_skill_but_automation_and_explicit_opt_out_never_prompt() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let initialized = in_terminal(&store, &["init"], "n\n", &[]);
    assert!(
        String::from_utf8_lossy(&initialized.stdout)
            .contains("Install the bundled Dalo assistant?")
    );
    assert!(!store.join("local/skills/dalo").exists());
    for arguments in [
        &["--json", "status"][..],
        &["--dry-run", "next"],
        &["status", "--check"],
        &["assistant", "status"],
        &["--help"],
        &["--version"],
    ] {
        let output = in_terminal(&store, arguments, "y\n", &[]);
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("[y/N]"),
            "{arguments:?}"
        );
        assert!(!store.join("local/skills/dalo").exists());
    }
    for environment in [[("DALO_ASSISTANT_CHECK", "never")], [("CI", "true")]] {
        let output = in_terminal(&store, &["next"], "y\n", &environment);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("[y/N]"));
    }
    dalo_command()
        .arg("--store")
        .arg(&store)
        .env("DALO_ASSISTANT_CHECK", "auto")
        .arg("next")
        .write_stdin("y\n")
        .assert()
        .success();
    assert!(!store.join("local/skills/dalo").exists());
}

#[test]
fn automatic_offer_preserves_customizations_and_foreign_entries_even_with_yes_ready() {
    for customized in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let store = temporary.path().join("store");
        let target = temporary.path().join("target");
        dalo_command()
            .arg("--store")
            .arg(&store)
            .arg("init")
            .assert()
            .success();
        dalo_command()
            .arg("--store")
            .arg(&store)
            .args(["target", "link", "generic"])
            .arg(&target)
            .assert()
            .success();
        let preserved = if customized {
            dalo_command()
                .arg("--store")
                .arg(&store)
                .args(["assistant", "install"])
                .assert()
                .success();
            store.join("local/skills/dalo/SKILL.md")
        } else {
            fs::create_dir(target.join("dalo")).unwrap();
            target.join("dalo/SKILL.md")
        };
        fs::write(&preserved, "# Keep my assistant").unwrap();
        let output = in_terminal(&store, &["next"], "y\n", &[]);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("[y/N]"));
        assert_eq!(
            fs::read_to_string(&preserved).unwrap(),
            "# Keep my assistant"
        );
        assert_eq!(
            assistant_status(&store)["state"],
            if customized { "blocked" } else { "external" }
        );
    }
}

#[test]
fn a_foreign_link_to_the_bundle_does_not_count_as_an_owned_delivery() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["assistant", "install"])
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "generic"])
        .arg(&first)
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "claude"])
        .arg(&second)
        .assert()
        .success();
    let foreign = second.join("dalo");
    symlink(store.join("local/skills/dalo"), &foreign).unwrap();
    let report = assistant_status(&store);
    assert_eq!(report["state"], "current");
    assert_eq!(report["external_paths"], serde_json::json!([foreign]));
    assert_eq!(report["undelivered_targets"], serde_json::json!(["claude"]));
    assert!(foreign.is_symlink());
}
