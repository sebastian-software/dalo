//! Offline lifecycle coverage for the released app-tracking client.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use app_tracking::client::Client;
use serde_json::Value;

mod common;

const ORIGIN: &str = "https://sebastian-app-tracking.b-cdn.net";

struct Fixture {
    root: tempfile::TempDir,
    state: PathBuf,
    store: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = root.path().join("store");
        Self { root, state, store }
    }

    fn command(&self) -> common::DaloCommand {
        let mut cmd = common::dalo_command();
        cmd.current_dir(self.root.path())
            .env("XDG_STATE_HOME", &self.state)
            .env("DALO_STORE", &self.store)
            .env("DALO_OFFLINE", "1")
            .env_remove("CI")
            .env_remove("CONTINUOUS_INTEGRATION")
            .env_remove("DO_NOT_TRACK")
            .env_remove("APP_TRACKING_TELEMETRY")
            .env_remove("APP_TRACKING_CI");
        cmd
    }

    fn allow(&self) {
        Client::registered(
            ORIGIN,
            self.state.join("dalo/app-tracking"),
            "dalo",
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap()
        .set_opt_in(true)
        .unwrap();
    }

    fn reporting_json(&self, action: &str) -> Value {
        json(self.command().args(["--json", "telemetry", action]))
    }

    fn installation(&self) -> Value {
        // These SDK state files belong only to the synthetic fixture.
        let service = fs::read_dir(self.state.join("dalo/app-tracking"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let state: Value =
            serde_json::from_slice(&fs::read(service.join("dalo/events-v1/state.json")).unwrap())
                .unwrap();
        state["identity"].clone()
    }
}

fn json(cmd: &mut assert_cmd::Command) -> Value {
    let result = cmd.assert().success().stderr("");
    serde_json::from_slice(&result.get_output().stdout).unwrap()
}

#[test]
fn no_consent_never_creates_usage_state_and_offline_opt_in_can_be_withdrawn() {
    let f = Fixture::new();
    f.command().arg("init").assert().success();
    assert_eq!(f.reporting_json("status")["decision"], "undecided");
    f.command().args(["telemetry", "enable"]).assert().failure();
    assert!(!f.state.exists());

    f.allow();
    for _ in 0..2 {
        json(f.command().args(["--json", "status"]));
    }
    f.command().args(["--dry-run", "sync"]).assert().success();
    f.command()
        .args([app_tracking::client::BACKGROUND_WORKER_ARGUMENT, "invalid"])
        .assert()
        .failure()
        .stdout("")
        .stderr("");
    let payload = f.reporting_json("preview");
    let rows = payload["open"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["dimensions"]["command"], "status");
    assert_eq!(rows[0]["count"], 2);
    assert!(rows[0]["project_id"].is_null());

    assert!(
        !f.command()
            .env("DO_NOT_TRACK", "1")
            .args(["--json", "status"])
            .assert()
            .success()
            .get_output()
            .stdout
            .is_empty()
    );
    assert_eq!(f.reporting_json("preview")["open"], serde_json::json!([]));
    json(f.command().args(["--json", "status"]));
    assert_eq!(f.reporting_json("disable")["decision"], "declined");
    json(f.command().args(["--json", "status"]));
    assert_eq!(f.reporting_json("preview")["open"], serde_json::json!([]));

    assert_eq!(f.reporting_json("delete")["deletion_pending"], true);
    assert_eq!(f.reporting_json("status")["deletion_pending"], true);
    assert_eq!(
        f.reporting_json("preview")["batches"],
        serde_json::json!([])
    );
}

#[test]
fn ci_requires_explicit_consent_and_state_isolated_from_personal_reporting() {
    let f = Fixture::new();
    f.allow();
    let ci_state = f.root.path().join("ci");
    fs::create_dir(&ci_state).unwrap();
    fs::set_permissions(&ci_state, fs::Permissions::from_mode(0o700)).unwrap();
    let ci = || {
        let mut cmd = f.command();
        cmd.env("CI", "1")
            .env("APP_TRACKING_CI", "telemetry-v1")
            .env("DALO_TELEMETRY_CI_STATE", &ci_state);
        cmd
    };
    ci().arg("init").assert().success();
    assert_eq!(
        json(ci().args(["--json", "telemetry", "status"]))["decision"],
        "undecided"
    );
    assert_eq!(fs::read_dir(&ci_state).unwrap().count(), 0);
    ci().env("DALO_TELEMETRY_CI_STATE", &f.state)
        .args(["telemetry", "enable-ci"])
        .assert()
        .failure();
    assert_eq!(
        json(ci().args(["--json", "telemetry", "enable-ci"]))["decision"],
        "granted"
    );
    json(ci().args(["--json", "status"]));
    let preview = json(ci().args(["--json", "telemetry", "preview"]));
    assert_eq!(preview["open"][0]["count"], 1);
    assert_eq!(f.reporting_json("preview")["open"], serde_json::json!([]));
}

fn git(f: &Fixture, dir: &Path, args: &[&str]) {
    let isolated = f.command();
    let mut command = std::process::Command::new("git");
    isolated.test_environment().apply_to(&mut command);
    assert!(
        command
            .current_dir(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn equivalent_raw_origins_share_one_project_hash_without_leaking_the_url() {
    let mut hashes = Vec::new();
    let mut installations = Vec::new();
    for origin in [
        "git@github.com:synthetic-org/synthetic-repo.git",
        "https://github.com/synthetic-org/synthetic-repo",
    ] {
        let f = Fixture::new();
        let project = f.root.path().join("repo");
        fs::create_dir(&project).unwrap();
        git(&f, &project, &["init", "-q"]);
        git(&f, &project, &["config", "remote.origin.url", origin]);
        git(
            &f,
            &project,
            &[
                "config",
                "url.https://mirror.invalid/.insteadOf",
                "git@github.com:",
            ],
        );
        f.allow();
        json(
            f.command()
                .env_remove("DALO_STORE")
                .args(["--json", "--project"])
                .arg(&project)
                .arg("init"),
        );
        let preview = f.reporting_json("preview");
        assert!(!preview.to_string().contains("synthetic-org"));
        let installation = f.installation();
        assert!(
            !preview
                .to_string()
                .contains(installation["token"].as_str().unwrap())
        );
        installations.push(installation);
        let hash = preview["open"][0]["project_id"]
            .as_str()
            .unwrap_or_else(|| panic!("preview omitted the project hash: {preview}"));
        assert_eq!(hash.len(), 64);
        hashes.push(hash.to_owned());
    }
    assert_eq!(hashes[0], hashes[1]);
    assert!(installations[0]["id"] != installations[1]["id"]);
    assert!(installations[0]["token"] != installations[1]["token"]);
}
