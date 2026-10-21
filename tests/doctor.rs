//! Focused integration checks for read-only doctor diagnostics.

mod common;

use common::{approve_source, dalo_command, git_rev_parse_logger, run_git};
use dalo::catalog;
use dalo::store::StorePaths;
use std::os::unix::fs::PermissionsExt;

fn commit(repo: &std::path::Path) {
    run_git(repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "-m",
            "initial",
            "-q",
        ],
    );
}

#[test]
fn doctor_should_resolve_each_nonlocal_source_head_once_across_diagnostics() {
    let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
    let store = temp_dir.path().join("store");
    let repository = temp_dir.path().join("catalog");
    dalo::store::init_store(store.clone(), false).expect("store should initialize");

    let skill = repository.join("skills/page-engine");
    std::fs::create_dir_all(&skill).expect("catalog skill directory should be created");
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: page-engine\ndescription: Page engine\nbinaries:\n  engine:\n    source: github-release\n    repo: example/engine\n    tag: v1.0.0\n    availability: required\n    assets:\n      macos-arm64:\n        asset: engine-macos-arm64\n        sha256: \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n      macos-x64:\n        asset: engine-macos-x64\n        sha256: \"1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n      linux-arm64:\n        asset: engine-linux-arm64\n        sha256: \"2123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n      linux-x64:\n        asset: engine-linux-x64\n        sha256: \"3123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n---\n# Page engine\n",
    )
    .expect("catalog skill should be written");

    let plugin = repository.join("plugins/quality");
    std::fs::create_dir_all(plugin.join("bin")).expect("plugin directory should be created");
    std::fs::write(
        plugin.join("PLUGIN.toml"),
        r#"schema_version = 1
[plugin]
name = "quality"
description = "Quality tools"

[[tool]]
schema_version = 1
id = "detector"
entry = "bin/detect"
runtime = "executable"
platforms = ["macos", "linux"]
argv = ["safe"]
cwd = "tool_root"
capabilities = ["filesystem_read"]
availability = "required"

[[hook]]
schema_version = 1
id = "protect-shell"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
matcher = { tool_names = ["Bash"] }
"#,
    )
    .expect("plugin manifest should be written");
    let entry = plugin.join("bin/detect");
    std::fs::write(&entry, "#!/bin/sh\nexit 0\n").expect("tool entry should be written");
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755))
        .expect("tool entry should be executable");
    commit(&repository);

    let paths = StorePaths::new(store.clone());
    catalog::add_catalog_source(
        &paths,
        "catalog",
        repository
            .to_str()
            .expect("repository path should be UTF-8"),
        None,
        false,
    )
    .expect("catalog source should be added");
    catalog::select_skills(&paths, "catalog", &["page-engine".to_owned()], false, false)
        .expect("catalog skill should be selected");
    approve_source(&store, "catalog");

    let git_logger = git_rev_parse_logger(temp_dir.path());
    let output = dalo_command()
        .args(["--store"])
        .arg(&store)
        .args(["--json", "doctor"])
        .env("PATH", &git_logger.path_env)
        .env("DALO_REAL_GIT", &git_logger.real_git)
        .env("DALO_GIT_REV_PARSE_LOG", &git_logger.log)
        .output()
        .expect("doctor should run");
    assert!(
        output.status.success(),
        "doctor should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("doctor JSON should parse");
    let findings = report["findings"]
        .as_array()
        .expect("doctor findings should be an array");
    let diagnostic_codes = findings
        .iter()
        .filter_map(|finding| {
            let code = finding["code"].as_str()?;
            matches!(
                code,
                "source_provenance_ok"
                    | "tool_pending_approval"
                    | "hook_tool_unavailable"
                    | "binary_pending_approval"
            )
            .then_some((code, finding["severity"].as_str()?))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostic_codes,
        [
            ("hook_tool_unavailable", "error"),
            ("binary_pending_approval", "warning"),
            ("tool_pending_approval", "warning"),
            ("source_provenance_ok", "ok"),
        ],
        "sharing HEAD lookups must preserve the doctor findings"
    );
    for (code, expected_message) in [
        (
            "binary_pending_approval",
            "binary `catalog:page-engine#binary:engine` is pending exact approval",
        ),
        (
            "hook_tool_unavailable",
            "hook `catalog:quality#hook:protect-shell`: referenced tool is not exactly approved, staged, and audited",
        ),
        (
            "tool_pending_approval",
            "tool `catalog:quality#tool:detector` is pending exact execution approval",
        ),
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding["code"] == code && finding["message"] == expected_message
            })
        );
    }
    assert!(findings.iter().any(|finding| {
        finding["code"] == "source_provenance_ok"
            && finding["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("catalog source `catalog` from "))
    }));

    let rev_parse_count = std::fs::read_to_string(&git_logger.log)
        .unwrap_or_default()
        .lines()
        .count();
    assert_eq!(
        rev_parse_count, 1,
        "doctor should run one git rev-parse HEAD per configured nonlocal source"
    );
}
