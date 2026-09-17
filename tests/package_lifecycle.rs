//! One package across content, instruction, execution, and provider ownership boundaries.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use dalo::hook::{self, HookProvider};
use dalo::{hook_dispatch, plugin_review, store};
use serde_json::{Value, json};

mod common;

const PACKAGE: &str = "local:review-suite";
const TOOL: &str = "local:review-suite#tool:context";
const HOOK: &str = "local:review-suite#hook:context";
const USER_NOTES: &str = "# Project\n\nUser-owned guidance.\n";

struct Fixture {
    _temp: tempfile::TempDir,
    paths: store::StorePaths,
    provider: HookProvider,
    provider_home: PathBuf,
    bin: PathBuf,
    target: PathBuf,
    instructions: PathBuf,
}

impl Fixture {
    fn new(provider: HookProvider) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = store::comparable_path(temp.path());
        let fixture = Self {
            paths: store::StorePaths::new(root.join("store")),
            provider,
            provider_home: root.join("provider"),
            bin: root.join("bin"),
            target: root.join("skills"),
            instructions: root.join(match provider {
                HookProvider::Claude => "CLAUDE.md",
                HookProvider::Codex => "AGENTS.md",
            }),
            _temp: temp,
        };
        fs::create_dir_all(&fixture.bin).unwrap();
        fs::create_dir_all(&fixture.provider_home).unwrap();
        executable(
            &fixture.bin.join(fixture.provider_name()),
            &format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", provider.baseline()),
        );
        fixture.command().arg("init").assert().success();
        fixture
            .command()
            .args(["target", "link", fixture.provider_name()])
            .arg(&fixture.target)
            .assert()
            .success();
        fs::create_dir_all(fixture.paths.local_dir.join("skills/review")).unwrap();
        fs::write(
            fixture.paths.local_dir.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: Review project changes.\n---\nReview carefully.\n",
        )
        .unwrap();
        fs::write(
            fixture.paths.local_dir.join("instructions/review-style.md"),
            "Keep reviews focused.\n",
        )
        .unwrap();
        fs::write(&fixture.instructions, USER_NOTES).unwrap();
        fs::write(
            fixture.sidecar(),
            serde_json::to_vec(&json!({"foreign": {"retained": true}})).unwrap(),
        )
        .unwrap();
        let package = fixture.paths.local_dir.join("plugins/review-suite");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("PLUGIN.toml"),
            r#"schema_version = 1
[plugin]
name = "review-suite"
description = "Complete portable package lifecycle fixture"
members = [
  { ref = "skill:review", requirement = "required" },
  { ref = "instruction:review-style", requirement = "recommended" },
]

[[tool]]
schema_version = 1
id = "context"
entry = "context"
runtime = "executable"
argv = []
cwd = "tool_root"
availability = "required"

[[hook]]
schema_version = 1
id = "context"
tool = "context"
subject = "user_prompt"
phase = "before"
effect = "add_context"
requirement = "required"
timeout_ms = 2000
failure_policy = "report"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
"#,
        )
        .unwrap();
        fixture.write_handler("Review version one.");
        fixture
    }

    fn provider_name(&self) -> &'static str {
        match self.provider {
            HookProvider::Claude => "claude",
            HookProvider::Codex => "codex",
        }
    }

    fn command(&self) -> common::DaloCommand {
        let mut command = common::dalo_command();
        let search = std::env::join_paths([&self.bin, &command.test_environment().path]).unwrap();
        command
            .args(["--store"])
            .arg(&self.paths.root)
            .env("PATH", search)
            .env("CODEX_HOME", &self.provider_home)
            .env("CLAUDE_CONFIG_DIR", &self.provider_home);
        command
    }

    fn sidecar(&self) -> PathBuf {
        self.provider_home.join(match self.provider {
            HookProvider::Claude => "settings.json",
            HookProvider::Codex => "hooks.json",
        })
    }

    fn write_handler(&self, context: &str) {
        let result = json!({"kind": "add_context", "context": context});
        executable(
            &self.paths.local_dir.join("plugins/review-suite/context"),
            &format!("#!/bin/sh\nprintf '%s' '{result}'\n"),
        );
    }

    fn approve_review(&self) {
        let report = plugin_review::build(&self.paths.root, PACKAGE).unwrap();
        let selected = report
            .decisions
            .iter()
            .filter(|decision| {
                matches!(
                    decision.kind,
                    plugin_review::ReviewDecisionKind::ToolExecution
                        | plugin_review::ReviewDecisionKind::HookBinding
                ) && matches!(
                    decision.state,
                    plugin_review::ReviewDecisionState::Pending
                        | plugin_review::ReviewDecisionState::Invalidated
                )
            })
            .map(|decision| decision.id.clone())
            .collect::<BTreeSet<_>>();
        assert!(
            !selected.is_empty(),
            "review must expose the changed execution boundary"
        );
        plugin_review::commit(&self.paths.root, PACKAGE, &report.review_token, &selected).unwrap();
    }

    fn assert_projection(&self, present: bool) {
        let sidecar: Value = serde_json::from_slice(&fs::read(self.sidecar()).unwrap()).unwrap();
        assert_eq!(sidecar["foreign"]["retained"], true);
        assert_eq!(sidecar.get("hooks").is_some(), present);
        let projection_dir = match self.provider {
            HookProvider::Claude => self.target.clone(),
            HookProvider::Codex => self.target.parent().unwrap().join("plugins/dalo"),
        };
        let count = fs::read_dir(projection_dir).map_or(0, |entries| {
            entries
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with("dalo-local-review-suite-")
                })
                .count()
        });
        assert_eq!(count, usize::from(present));
    }

    fn dispatch(&self) -> Value {
        self.dispatch_event(&json!({"session_id": "lifecycle", "prompt": "Review"}))
    }

    fn dispatch_event(&self, input: &Value) -> Value {
        let projection = hook::compile_native_projection(
            &self.paths,
            self.provider,
            self.provider.baseline(),
            Path::new(assert_cmd::cargo::cargo_bin!("dalo")),
            &[hook::show(&self.paths, HOOK).unwrap()],
        )
        .unwrap();
        let group = &projection.dispatcher_manifest["groups"][0];
        hook_dispatch::dispatch(
            &self.paths,
            &hook_dispatch::DispatchRequest {
                provider: self.provider,
                projection: &projection.fingerprint,
                event: "UserPromptSubmit",
                group: group["id"].as_str().unwrap(),
            },
            &serde_json::to_vec(input).unwrap(),
        )
        .unwrap()
    }
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn package_lifecycle_preserves_independent_trust_and_user_content() {
    for provider in [HookProvider::Claude, HookProvider::Codex] {
        let fixture = Fixture::new(provider);
        fixture
            .command()
            .args(["plugin", "select", PACKAGE])
            .assert()
            .success();
        let approvals = fs::read(fixture.paths.root.join("approvals.toml")).unwrap();
        fixture
            .command()
            .args(["--json", "plugin", "review", PACKAGE])
            .assert()
            .success();
        fixture
            .command()
            .args(["--dry-run", "sync"])
            .assert()
            .success();
        assert_eq!(
            fs::read(fixture.paths.root.join("approvals.toml")).unwrap(),
            approvals
        );
        assert_eq!(
            fs::read_to_string(&fixture.instructions).unwrap(),
            USER_NOTES
        );
        fixture.assert_projection(false);

        fixture.approve_review();
        let sidecar_before = fs::read(fixture.sidecar()).unwrap();
        fixture
            .command()
            .args(["--dry-run", "sync"])
            .assert()
            .success();
        assert_eq!(fs::read(fixture.sidecar()).unwrap(), sidecar_before);
        fixture.command().arg("sync").assert().success();
        fixture.assert_projection(true);
        assert!(fixture.target.join("review/SKILL.md").is_file());
        assert_eq!(
            fixture.dispatch()["hookSpecificOutput"]["additionalContext"],
            "Review version one."
        );
        assert_eq!(
            fs::read_to_string(&fixture.instructions).unwrap(),
            USER_NOTES
        );

        fixture
            .command()
            .args(["instructions", "enable", "review-style"])
            .arg(&fixture.instructions)
            .assert()
            .success();
        let enabled = fs::read_to_string(&fixture.instructions).unwrap();
        assert!(enabled.starts_with(USER_NOTES));
        assert!(enabled.contains("Keep reviews focused."));

        // Updating executable bytes invalidates both the tool and its bound hook.
        fixture.write_handler("Review version two.");
        fixture.command().arg("sync").assert().success();
        fixture.assert_projection(false);
        assert_eq!(fs::read_to_string(&fixture.instructions).unwrap(), enabled);
        fixture.approve_review();
        fixture.command().arg("sync").assert().success();
        fixture.assert_projection(true);
        assert_eq!(
            fixture.dispatch()["hookSpecificOutput"]["additionalContext"],
            "Review version two."
        );

        // Either execution boundary independently removes the native activation.
        for (scope, identity) in [("hook", HOOK), ("tool", TOOL)] {
            fixture
                .command()
                .args(["approve", "revoke", scope, identity])
                .assert()
                .success();
            fixture.command().arg("sync").assert().success();
            fixture.assert_projection(false);
            assert_eq!(fs::read_to_string(&fixture.instructions).unwrap(), enabled);
            fixture.approve_review();
            fixture.command().arg("sync").assert().success();
            fixture.assert_projection(true);
        }

        fixture
            .command()
            .args(["plugin", "unselect", PACKAGE])
            .assert()
            .success();
        fixture.command().arg("sync").assert().success();
        fixture.assert_projection(false);
        // Instruction activation is independent of package selection.
        assert_eq!(fs::read_to_string(&fixture.instructions).unwrap(), enabled);
        fixture
            .command()
            .args(["instructions", "disable", "review-style"])
            .arg(&fixture.instructions)
            .assert()
            .success();
        // The separator inserted before the managed block remains outside its
        // ownership; disabling removes the block and preserves that whitespace.
        assert_eq!(
            fs::read_to_string(&fixture.instructions).unwrap(),
            format!("{USER_NOTES}\n")
        );
    }
}

/// A hook handler may ignore its event and exit at once, and the dispatcher
/// prewrites the payload to a seekable file for exactly that reason. Sending it
/// through a child stdin pipe instead races that exit: the write fails with
/// `EPIPE` once the payload outgrows the pipe buffer, and a handler that did
/// its work is reported as failed.
#[test]
fn dispatch_hands_an_oversized_event_to_a_handler_that_never_reads_stdin() {
    let fixture = Fixture::new(HookProvider::Claude);
    fixture
        .command()
        .args(["plugin", "select", PACKAGE])
        .assert()
        .success();
    fixture.approve_review();
    fixture.command().arg("sync").assert().success();
    let prompt = "prompt ".repeat(64 * 1024);
    assert!(prompt.len() > 64 * 1024, "the event must outgrow a pipe");
    let actual = fixture.dispatch_event(&json!({"session_id": "lifecycle", "prompt": prompt}));
    assert_eq!(
        actual["hookSpecificOutput"]["additionalContext"], "Review version one.",
        "a handler that never reads its event must still succeed: {actual}"
    );
}
