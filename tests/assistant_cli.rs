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
fn first_use_bootstraps_without_init_and_inspection_reports_the_matching_bundle() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let bare = in_terminal(&store, &[], "", &[]);
    assert!(!String::from_utf8_lossy(&bare.stdout).contains("[y/N]"));
    assert_eq!(assistant_status(&store)["state"], "current");
    make_bundle_stale(&store);
    let receipt = store.join("local/skills/dalo/.dalo-bundle.toml");
    let before = fs::read(&receipt).unwrap();
    for arguments in [
        &["--dry-run", "init"][..],
        &["assistant", "status"],
        &["--help"],
        &["--version"],
    ] {
        let output = in_terminal(&store, arguments, "", &[]);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("[y/N]"));
        assert_eq!(fs::read(&receipt).unwrap(), before, "{arguments:?}");
    }
    in_terminal(&store, &["next"], "", &[]);
    assert_eq!(assistant_status(&store)["state"], "current");
}

#[test]
fn automatic_setup_opt_out_preserves_explicit_installation() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    in_terminal(&store, &["init"], "", &[("DALO_ASSISTANT_CHECK", "never")]);
    assert_eq!(assistant_status(&store)["state"], "missing");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["assistant", "install"])
        .assert()
        .success();
    assert_eq!(assistant_status(&store)["state"], "current");
}

#[test]
fn read_only_calls_preserve_customizations_and_foreign_entries() {
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

fn automatic_command(store: &std::path::Path, arguments: &[&str]) -> Value {
    let output = dalo_command()
        .arg("--store")
        .arg(store)
        .env_remove("DALO_ASSISTANT_CHECK")
        .env("DALO_OFFLINE", "1")
        .arg("--json")
        .args(arguments)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

#[test]
fn json_setup_installs_and_delivers_without_a_separate_request_or_general_sync() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("target");
    let initialized = automatic_command(&store, &["init"]);
    assert_eq!(initialized["assistant"]["state"], "current");
    fs::create_dir_all(store.join("local/skills/unrelated")).unwrap();
    fs::write(
        store.join("local/skills/unrelated/SKILL.md"),
        "# Unrelated\n",
    )
    .unwrap();
    let config = fs::read(store.join("config.toml")).unwrap();
    let approvals = fs::read(store.join("approvals.toml")).unwrap();
    let lock = fs::read(store.join("lock.toml")).unwrap();
    let linked = dalo_command()
        .arg("--store")
        .arg(&store)
        .env("DALO_ASSISTANT_CHECK", "auto")
        .env("DALO_OFFLINE", "1")
        .env("PATH", temporary.path().join("no-programs"))
        .args(["--json", "target", "link", "generic"])
        .arg(&target)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let linked: Value = serde_json::from_slice(&linked).unwrap();
    assert_eq!(linked["assistant"]["state"], "current");
    assert_eq!(
        linked["assistant"]["undelivered_targets"],
        serde_json::json!([])
    );
    assert!(target.join("dalo/SKILL.md").is_file());
    assert!(target.join("dalo/references/setup.md").is_file());
    assert!(!target.join("unrelated").exists());
    assert_eq!(fs::read(store.join("config.toml")).unwrap(), config);
    assert_eq!(fs::read(store.join("approvals.toml")).unwrap(), approvals);
    let updated_lock: dalo::lockfile::UserLock =
        toml::from_str(&fs::read_to_string(store.join("lock.toml")).unwrap()).unwrap();
    let previous_lock: dalo::lockfile::UserLock =
        toml::from_str(std::str::from_utf8(&lock).unwrap()).unwrap();
    assert_eq!(updated_lock.plugins, previous_lock.plugins);
    assert_eq!(
        updated_lock.pending_approval_skills,
        previous_lock.pending_approval_skills
    );
    assert_eq!(updated_lock.active_skills.len(), 1);
    assert_eq!(updated_lock.active_skills[0].source_ref, "local:dalo");
    let repeated = automatic_command(
        &store,
        &["target", "link", "generic", target.to_str().unwrap()],
    );
    assert_eq!(repeated["status"], "existing");
    assert_eq!(repeated["assistant"]["state"], "current");
    let state = dalo::store::read_state(&dalo::store::StorePaths::new(store)).unwrap();
    assert_eq!(state.owned_skills.len(), 1);
}

#[test]
fn automatic_target_setup_preserves_other_owned_links_and_repairs_only_the_companion() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    automatic_command(&store, &["init"]);
    dalo_command()
        .arg("--store")
        .arg(&store)
        .args(["target", "link", "claude"])
        .arg(&first)
        .assert()
        .success();
    fs::create_dir_all(store.join("local/skills/other")).unwrap();
    fs::write(store.join("local/skills/other/SKILL.md"), "# Other\n").unwrap();
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    fs::remove_file(first.join("dalo")).unwrap();
    let other_link = fs::read_link(first.join("other")).unwrap();
    let paths = dalo::store::StorePaths::new(store.clone());
    let before = dalo::store::read_state(&paths).unwrap();
    automatic_command(
        &store,
        &["target", "link", "generic", second.to_str().unwrap()],
    );
    assert!(second.join("dalo").is_symlink());
    assert!(!second.join("other").exists());
    assert!(first.join("dalo").exists());
    assert_eq!(fs::read_link(first.join("other")).unwrap(), other_link);
    let after = dalo::store::read_state(&paths).unwrap();
    for previous in before.owned_skills {
        assert!(after.owned_skills.contains(&previous));
    }
}

#[test]
fn automatic_setup_previews_do_not_write_and_ci_setup_does_not_prompt() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("target");
    automatic_command(&store, &["--dry-run", "init"]);
    assert!(!store.exists());
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    let state = fs::read(store.join("state.toml")).unwrap();
    automatic_command(
        &store,
        &[
            "--dry-run",
            "target",
            "link",
            "generic",
            target.to_str().unwrap(),
        ],
    );
    assert!(!target.exists());
    assert!(!store.join("local/skills/dalo").exists());
    assert_eq!(fs::read(store.join("state.toml")).unwrap(), state);
    dalo_command()
        .arg("--store")
        .arg(&store)
        .env("DALO_ASSISTANT_CHECK", "auto")
        .env("CI", "true")
        .args(["--json", "init"])
        .assert()
        .success();
    assert_eq!(assistant_status(&store)["state"], "current");
}

#[test]
fn sync_repairs_missing_bundle_and_updates_only_unmodified_bundles() {
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
    let preview = automatic_command(&store, &["--dry-run", "sync"]);
    assert!(preview["dry_run"].as_bool().unwrap());
    assert!(!store.join("local/skills/dalo").exists());
    let synced = automatic_command(&store, &["sync", "--check"]);
    assert_eq!(synced["assistant"]["state"], "current");
    assert!(target.join("dalo").is_symlink());
    make_bundle_stale(&store);
    automatic_command(&store, &["sync", "--check"]);
    assert_eq!(assistant_status(&store)["state"], "current");
    let notes = store.join("local/skills/dalo/notes.md");
    fs::write(&notes, "Keep these notes\n").unwrap();
    let customized = automatic_command(&store, &["sync", "--check"]);
    assert_eq!(customized["assistant"]["state"], "blocked");
    assert_eq!(fs::read_to_string(notes).unwrap(), "Keep these notes\n");
}

#[test]
fn automatic_setup_preserves_foreign_slots_and_unowned_links_to_the_bundle() {
    for same_bundle in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let store = temporary.path().join("store");
        let target = temporary.path().join("target");
        automatic_command(&store, &["init"]);
        fs::create_dir(&target).unwrap();
        if same_bundle {
            symlink(store.join("local/skills/dalo"), target.join("dalo")).unwrap();
        } else {
            fs::create_dir(target.join("dalo")).unwrap();
            fs::write(target.join("dalo/SKILL.md"), "# Existing\n").unwrap();
        }
        let linked = automatic_command(
            &store,
            &["target", "link", "generic", target.to_str().unwrap()],
        );
        assert_eq!(linked["assistant"]["state"], "blocked");
        let state = dalo::store::read_state(&dalo::store::StorePaths::new(store)).unwrap();
        assert!(state.owned_skills.is_empty());
        if same_bundle {
            assert!(target.join("dalo").is_symlink());
        } else {
            assert_eq!(
                fs::read_to_string(target.join("dalo/SKILL.md")).unwrap(),
                "# Existing\n"
            );
        }
    }
}

#[test]
fn project_install_delivers_the_bundle_only_inside_the_project() {
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(
        project.join("dalo-project.toml"),
        "schema_version = 1\ntargets = [\"codex\"]\n",
    )
    .unwrap();
    let mut command = dalo_command();
    let home = command.test_environment().home.clone();
    let output = command
        .arg("--project")
        .arg(&project)
        .env("DALO_ASSISTANT_CHECK", "auto")
        .env("DALO_OFFLINE", "1")
        .args(["--json", "install"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(output["assistant"]["state"], "current");
    assert!(project.join(".agents/skills/dalo/SKILL.md").is_file());
    assert!(!home.join(".agents/skills").exists());
    assert!(!home.join(".dalo").exists());
    assert!(
        fs::read_to_string(project.join(".dalo/approvals.toml"))
            .unwrap()
            .contains("approvals = []")
    );
}

#[test]
fn automatic_setup_does_not_shadow_a_configured_assistant_source() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let foreign = temporary.path().join("foreign");
    dalo_command()
        .arg("--store")
        .arg(&store)
        .arg("init")
        .assert()
        .success();
    fs::create_dir_all(foreign.join("skills/dalo")).unwrap();
    fs::write(
        foreign.join("skills/dalo/SKILL.md"),
        "---\nname: dalo\ndescription: Existing assistant\n---\n# Existing assistant\n",
    )
    .unwrap();
    let paths = dalo::store::StorePaths::new(store.clone());
    let mut config = dalo::store::read_config(&paths).unwrap();
    let mut source = config.sources[0].clone();
    source.id = "existing".to_owned();
    source.kind = dalo::source::SourceKind::Team;
    source.path = foreign;
    source.priority = 1;
    config.sources.push(source);
    dalo::store::write_config(&paths, &config).unwrap();
    let report = automatic_command(&store, &["init"]);
    assert_eq!(report["assistant"]["state"], "external");
    assert!(!store.join("local/skills/dalo").exists());
}

#[test]
fn corrupt_lock_blocks_automatic_bundle_updates() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("target");
    automatic_command(&store, &["init"]);
    make_bundle_stale(&store);
    let receipt = store.join("local/skills/dalo/.dalo-bundle.toml");
    let before = fs::read(&receipt).unwrap();
    fs::write(store.join("lock.toml"), "broken = [").unwrap();
    let linked = automatic_command(
        &store,
        &["target", "link", "generic", target.to_str().unwrap()],
    );
    assert_eq!(linked["assistant"]["state"], "blocked");
    assert_eq!(fs::read(&receipt).unwrap(), before);
    assert!(!target.join("dalo").exists());
    assert_eq!(
        fs::read_to_string(store.join("lock.toml")).unwrap(),
        "broken = ["
    );
}

#[test]
fn first_default_use_delivers_only_the_packaged_companion_to_existing_agents() {
    let seed = dalo_command();
    let home = seed.test_environment().home.clone();
    let codex = home.join(".agents/skills");
    let claude = home.join(".claude/skills");
    fs::create_dir_all(&codex).unwrap();
    fs::create_dir_all(&claude).unwrap();
    let run = |arguments: &[&str]| {
        dalo_command()
            .env("HOME", &home)
            .env_remove("DALO_ASSISTANT_CHECK")
            .env("DALO_OFFLINE", "1")
            .args(arguments)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone()
    };
    let first: Value =
        serde_json::from_slice(&run(&["--global", "--json", "status", "--check"])).unwrap();
    assert_eq!(first["assistant"]["state"], "current");
    assert_eq!(
        first["assistant"]["delivered_paths"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(first["targets"], serde_json::json!([]));
    let store = home.join(".dalo");
    let paths = dalo::store::StorePaths::new(store.clone());
    assert!(dalo::store::read_state(&paths).unwrap().targets.is_empty());
    assert!(codex.join("dalo/references/setup.md").is_file());
    assert!(claude.join("dalo/references/maintenance.md").is_file());
    let receipt = fs::read(store.join("assistant-links.toml")).unwrap();
    fs::remove_file(codex.join("dalo")).unwrap();
    run(&["--global", "--json", "status", "--check"]);
    assert!(codex.join("dalo").is_symlink());
    assert_eq!(
        fs::read(store.join("assistant-links.toml")).unwrap(),
        receipt
    );
    make_bundle_stale(&store);
    run(&["--global", "next"]);
    assert_eq!(assistant_status(&store)["state"], "current");
    fs::create_dir_all(store.join("local/skills/other")).unwrap();
    fs::write(store.join("local/skills/other/SKILL.md"), "# Other\n").unwrap();
    run(&["--global", "--json", "sync"]);
    assert!(!codex.join("other").exists());
    assert!(!claude.join("other").exists());
    assert!(codex.join("dalo").is_symlink());
    assert!(claude.join("dalo").is_symlink());
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "--json", "status", "--check"])
        .assert()
        .code(1);
}

#[test]
fn first_use_opt_out_and_dry_run_leave_a_missing_store_untouched() {
    for arguments in [
        &["--global", "--dry-run", "status"][..],
        &["--global", "assistant", "status"],
        &["--global", "plan"],
        &["--help"],
        &["--version"],
    ] {
        let mut command = dalo_command();
        let store = command.test_environment().home.join(".dalo");
        command
            .env_remove("DALO_ASSISTANT_CHECK")
            .args(arguments)
            .output()
            .unwrap();
        assert!(!store.exists(), "{arguments:?}");
    }
    let mut command = dalo_command();
    let store = command.test_environment().home.join(".dalo");
    command.args(["--global", "next"]).assert().success();
    assert!(!store.exists());
}

#[test]
fn first_use_preserves_custom_bundles_and_does_not_claim_foreign_companion_links() {
    let seed = dalo_command();
    let home = seed.test_environment().home.clone();
    let target = home.join(".agents/skills");
    fs::create_dir_all(&target).unwrap();
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "next"])
        .assert()
        .success();
    let store = home.join(".dalo");
    let skill = store.join("local/skills/dalo/SKILL.md");
    fs::write(&skill, "# My changes\n").unwrap();
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "next"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(&skill).unwrap(), "# My changes\n");
    fs::remove_file(target.join("dalo")).unwrap();
    let foreign = home.join("foreign");
    fs::create_dir(&foreign).unwrap();
    symlink(&foreign, target.join("dalo")).unwrap();
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "next"])
        .assert()
        .success();
    assert_eq!(fs::read_link(target.join("dalo")).unwrap(), foreign);
}

#[test]
fn newer_companion_receipt_blocks_updates_without_overwriting_recovery_data() {
    let seed = dalo_command();
    let home = seed.test_environment().home.clone();
    fs::create_dir_all(home.join(".agents/skills")).unwrap();
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "next"])
        .assert()
        .success();
    let store = home.join(".dalo");
    make_bundle_stale(&store);
    let bundle_receipt = fs::read(store.join("local/skills/dalo/.dalo-bundle.toml")).unwrap();
    let newer = "schema_version = 999\nlinks = []\n";
    fs::write(store.join("assistant-links.toml"), newer).unwrap();
    dalo_command()
        .env("HOME", &home)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--global", "next"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(store.join("assistant-links.toml")).unwrap(),
        newer
    );
    assert_eq!(
        fs::read(store.join("local/skills/dalo/.dalo-bundle.toml")).unwrap(),
        bundle_receipt
    );
    assert_eq!(assistant_status(&store)["state"], "blocked");
}

#[test]
fn first_use_of_an_explicit_custom_store_does_not_install_global_companions() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("sandbox-store");
    let mut command = dalo_command();
    let target = command.test_environment().home.join(".agents/skills");
    fs::create_dir_all(&target).unwrap();
    command
        .arg("--store")
        .arg(&store)
        .env_remove("DALO_ASSISTANT_CHECK")
        .args(["--json", "status"])
        .assert()
        .success();
    assert!(store.join("local/skills/dalo/SKILL.md").is_file());
    assert!(!target.join("dalo").exists());
    assert!(!store.join("assistant-links.toml").exists());
}

/// The global store's own companion links look like another installer's
/// assistant from a sandbox store. That standing state must not be narrated on
/// every command there; it stays visible on request.
#[test]
fn custom_stores_keep_an_external_assistant_out_of_normal_output() {
    let temporary = tempfile::tempdir().unwrap();
    let store = temporary.path().join("store");
    let target = temporary.path().join("skills");
    let seed = dalo_command();
    let environment = seed.test_environment();
    let foreign = environment.home.join(".agents/skills/dalo");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("SKILL.md"), "# Installed by another manager\n").unwrap();
    let run = |arguments: &[&str]| {
        let output = environment
            .command()
            .env_remove("DALO_ASSISTANT_CHECK")
            .env("DALO_OFFLINE", "1")
            .arg("--store")
            .arg(&store)
            .args(arguments)
            .assert()
            .success()
            .get_output()
            .clone();
        (
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    };
    let target_argument = target.to_str().unwrap();
    for arguments in [
        &["init"][..],
        &["target", "link", "generic", target_argument],
        &["status"],
        &["sync"],
        &["sync"],
        &["next"],
        &["doctor"],
    ] {
        let (stdout, stderr) = run(arguments);
        assert!(
            !stderr.contains("assistant:") && !stdout.contains("assistant:"),
            "{arguments:?} narrated the assistant:\n{stdout}\n{stderr}"
        );
    }

    let (status, _) = run(&["--json", "status"]);
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["assistant"]["state"], "external");
    let (doctor, _) = run(&["--json", "doctor"]);
    let doctor: Value = serde_json::from_str(&doctor).unwrap();
    let findings = doctor["findings"].as_array().unwrap();
    let external = findings
        .iter()
        .filter(|finding| finding["code"] == "assistant_external")
        .collect::<Vec<_>>();
    assert_eq!(external.len(), 1, "{findings:?}");
    assert_eq!(external[0]["severity"], "info");
    let (doctor_text, _) = run(&["doctor"]);
    assert!(
        doctor_text.contains("info    assistant_external: "),
        "{doctor_text}"
    );
    assert!(!store.join(".assistant-notice").exists());
}

/// In the global store the same state is real news, so it is said once and
/// remembered, and resolving it clears the record for the next occurrence.
#[test]
fn the_global_store_reports_an_external_assistant_once() {
    let seed = dalo_command();
    let environment = seed.test_environment();
    let foreign = environment.home.join(".agents/skills/dalo");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("SKILL.md"), "# Installed by another manager\n").unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let run = |arguments: &[&str]| {
        let output = environment
            .command()
            .current_dir(workspace.path())
            .env_remove("DALO_ASSISTANT_CHECK")
            .env("DALO_OFFLINE", "1")
            .arg("--global")
            .args(arguments)
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(output.stderr).unwrap()
    };

    let first = run(&["init"]);
    assert_eq!(
        first
            .matches(
                "assistant: An assistant already exists outside this store's bundled installation"
            )
            .count(),
        1,
        "{first}"
    );
    let marker = environment.home.join(".dalo/.assistant-notice");
    assert!(marker.is_file());
    for arguments in [&["status"][..], &["sync"], &["next"], &["sync"]] {
        let stderr = run(arguments);
        assert!(!stderr.contains("assistant:"), "{arguments:?}: {stderr}");
    }

    fs::remove_dir_all(&foreign).unwrap();
    let resolved = run(&["status"]);
    assert!(
        resolved
            .contains("assistant: prepared the bundled Dalo skill in ~/.dalo/local/skills/dalo"),
        "{resolved}"
    );
    assert!(
        resolved.contains("assistant: delivered to ~/.agents/skills/dalo;"),
        "{resolved}"
    );
    assert!(!marker.exists());
    let settled = run(&["status"]);
    assert!(!settled.contains("assistant:"), "{settled}");
}
