//! Proof that a store written by an older released Dalo opens in the current
//! one without manual steps.
//!
//! Every fixture under `tests/fixtures/stores/` was written by a released
//! binary, not by this tree. Each test restores one into a temporary root and
//! runs the commands a person actually runs after upgrading: `status`,
//! `doctor --check`, `sync --dry-run`, `sync`, and `status --json`. Nothing here
//! touches the network or `$HOME`; the fixture Git repositories travel with the
//! fixture as bare copies.
//!
//! See `tests/fixtures/stores/README.md` for how the fixtures were built and
//! what has to be relocated.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

mod common;

use common::dalo_command;

/// Versions covered below. A fixture directory without an entry here fails
/// `every_fixture_is_covered`, so adding a fixture cannot silently add nothing.
const COVERED: [&str; 4] = ["0.6.0", "0.9.2", "0.12.0", "0.15.1"];

#[test]
fn store_written_by_0_6_0_upgrades_without_manual_steps() {
    let upgraded = upgrade("0.6.0");
    // 0.6.0 predates `dalo approve`, so its bare `launch-copy` record could only
    // be hand-authored. It was already ignored in 0.6.0 and is still ignored;
    // what the upgrade must do is say so and name the exact command.
    upgraded.assert_migration_needs_command(
        "legacy approval `launch-copy` found for `public:launch-copy`; re-approve as `public:launch-copy`",
        "approve skill public:launch-copy",
    );
    upgraded.assert_migrations_reported(&[
        "`config.toml` is at schema version 1 and is read as 2",
        "`source-lock.toml` is at schema version 2 and is read as 3",
    ]);
}

#[test]
fn store_written_by_0_9_2_upgrades_without_manual_steps() {
    let upgraded = upgrade("0.9.2");
    upgraded.assert_migrations_reported(&["`config.toml` is at schema version 1 and is read as 2"]);
}

#[test]
fn store_written_by_0_12_0_upgrades_without_manual_steps() {
    upgrade("0.12.0");
}

#[test]
fn store_written_by_0_15_1_upgrades_without_manual_steps() {
    let upgraded = upgrade("0.15.1");
    // The newest fixture is the control: every schema is already current, so a
    // regression that starts migrating something unnecessarily shows up here.
    assert!(
        upgraded.pending_migrations().is_empty(),
        "0.15.1 store should need no migration, reported: {:?}",
        upgraded.pending_migrations()
    );
}

#[test]
fn every_fixture_is_covered() {
    let mut found = BTreeSet::new();
    for entry in std::fs::read_dir(fixture_root()).expect("fixture root should be readable") {
        let entry = entry.expect("fixture directory entry should be readable");
        if entry.path().join("fixture.toml").is_file() {
            found.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }

    assert_eq!(
        found,
        COVERED.iter().map(|&version| version.to_owned()).collect(),
        "every fixture under tests/fixtures/stores needs an upgrade test"
    );
}

/// Run the full upgrade sequence against one fixture and assert the invariants
/// that hold for every fixture, whatever it happens to contain.
fn upgrade(version: &str) -> Upgraded {
    let restored = Restored::new(version);
    let fixture = &restored.fixture;

    // 1. `status` before anything is written: the resolved set the older binary
    //    recorded must survive being read by this one.
    let status = restored.json(&["status", "--json"]);
    assert_eq!(
        source_refs(&status["resolution"]["active_skills"]),
        fixture.recorded.active_skills,
        "{version}: active skills changed on upgrade"
    );
    assert_eq!(
        source_refs(&status["resolution"]["pending_approval_skills"]),
        fixture.recorded.pending_approval_skills,
        "{version}: upgrade introduced or dropped a pending approval"
    );

    // 2. `doctor --check` must not find an error in a store that was healthy.
    let doctor = restored.json(&["doctor", "--check", "--json"]);
    assert_eq!(
        doctor["summary"]["errors"],
        0,
        "{version}: doctor reported errors: {:#?}",
        findings_with_severity(&doctor, "error")
    );

    // 3. `sync --dry-run` and 4. `sync` must leave every owned link alone.
    for arguments in [
        vec!["sync", "--dry-run", "--json"],
        vec!["sync", "--json"],
        vec!["sync", "--json"],
    ] {
        let sync = restored.json(&arguments);
        restored.assert_no_link_removed(&sync, &arguments.join(" "));
    }

    // The second `sync` above must additionally be a pure no-op.
    let second = restored.json(&["sync", "--json"]);
    for operation in second["operations"]
        .as_array()
        .expect("sync operations should be an array")
    {
        assert_eq!(
            operation["kind"], "no_op",
            "{version}: a repeated sync is not a no-op: {operation:#?}"
        );
    }

    // Every owned link the fixture had is still a symlink to the same relative
    // place in the relocated store.
    for link in &fixture.owned_links {
        let path = restored.root.join("target").join(&link.name);
        let resolved = std::fs::read_link(&path).unwrap_or_else(|error| {
            panic!("{version}: owned link `{}` is gone: {error}", link.name)
        });
        assert_eq!(
            resolved,
            restored.root.join(&link.target),
            "{version}: owned link `{}` was repointed",
            link.name
        );
    }

    // The unmanaged directory the fixture protected is untouched.
    let protected = restored.root.join("target/keep-mine/SKILL.md");
    assert!(
        protected.is_file(),
        "{version}: upgrade touched the protected unmanaged skill"
    );

    // Everything `sync` writes carries the current schema version afterwards.
    // `config.toml` and `source-lock.toml` are written by other commands, so
    // they migrate on their own next write; `doctor` reports that (see below).
    assert_eq!(
        restored.persisted_version("lock.toml", "schema_version"),
        Some(dalo::lockfile::USER_LOCK_SCHEMA_VERSION),
        "{version}: sync left lock.toml on an old schema"
    );
    assert_eq!(
        restored.persisted_version("state.toml", "schema_version"),
        Some(dalo::store::STATE_SCHEMA_VERSION),
        "{version}: sync left state.toml on an old schema"
    );
    assert_eq!(
        restored.persisted_version("approvals.toml", "schema_version"),
        Some(dalo::store::APPROVALS_SCHEMA_VERSION),
        "{version}: approvals.toml is on an old schema"
    );

    // A path-only protection record is rewritten as a target slot, so a later
    // `sync` cannot mistake the slot for an orphan and unlink it.
    let state = std::fs::read_to_string(restored.store().join("state.toml"))
        .expect("migrated state should be readable");
    assert!(
        !state.contains("keep-mine\"\npath = ") && state.contains("target_id = \"generic\""),
        "{version}: protected skill was not migrated to a target slot:\n{state}"
    );

    // The rendered instruction block still belongs to the pack that owns it.
    let instructions = std::fs::read_to_string(restored.root.join("instructions/AGENTS.md"))
        .expect("instruction file should be readable");
    assert!(
        instructions.contains("<!-- dalo:start house-style -->")
            && instructions.contains("Write in English. Prefer short sentences."),
        "{version}: instruction block was lost:\n{instructions}"
    );

    let final_doctor = restored.json(&["doctor", "--check", "--json"]);
    assert_eq!(
        final_doctor["summary"]["errors"],
        0,
        "{version}: doctor reported errors after sync: {:#?}",
        findings_with_severity(&final_doctor, "error")
    );

    Upgraded {
        version: version.to_owned(),
        doctor: final_doctor,
        restored,
    }
}

/// A fixture that has been through the upgrade sequence.
struct Upgraded {
    version: String,
    doctor: serde_json::Value,
    restored: Restored,
}

impl Upgraded {
    /// Migration lines `doctor` still reports after the upgrade.
    fn pending_migrations(&self) -> Vec<String> {
        self.doctor["findings"]
            .as_array()
            .expect("doctor findings should be an array")
            .iter()
            .filter(|finding| finding["code"] == "schema_migration_pending")
            .map(|finding| {
                finding["message"]
                    .as_str()
                    .expect("finding message should be a string")
                    .to_owned()
            })
            .collect()
    }

    /// Assert that doctor reports one line per expected pending migration.
    fn assert_migrations_reported(&self, expected: &[&str]) {
        let reported = self.pending_migrations();
        for line in expected {
            assert!(
                reported.iter().any(|message| message.contains(line)),
                "{}: doctor did not report the pending migration `{line}`; it reported {reported:#?}",
                self.version
            );
        }
    }

    /// Assert the exact message and next command for a migration that a person
    /// has to finish. Automatic migrations carry no command; this one must.
    ///
    /// `arguments` is the command tail; the expected string is built the way
    /// Dalo builds it, so a non-default store is qualified with `--store`.
    fn assert_migration_needs_command(&self, message: &str, arguments: &str) {
        let command = dalo::store::dalo_command(&self.restored.store(), arguments);
        let finding = self.doctor["findings"]
            .as_array()
            .expect("doctor findings should be an array")
            .iter()
            .find(|finding| finding["message"] == message)
            .unwrap_or_else(|| {
                panic!(
                    "{}: doctor did not report `{message}`; findings: {:#?}",
                    self.version, self.doctor["findings"]
                )
            });
        assert_eq!(
            finding["code"], "legacy_approval_record",
            "{}: expected the legacy-approval code",
            self.version
        );
        assert_eq!(
            finding["next_command"], command,
            "{}: expected the exact command a person must run",
            self.version
        );
    }
}

/// A fixture copied to a temporary root with every recorded path rewritten.
struct Restored {
    _temp: tempfile::TempDir,
    root: PathBuf,
    fixture: Fixture,
}

impl Restored {
    fn new(version: &str) -> Self {
        let source = fixture_root().join(version);
        let fixture: Fixture = toml::from_str(
            &std::fs::read_to_string(source.join("fixture.toml"))
                .expect("fixture description should be readable"),
        )
        .expect("fixture description should parse");

        let temp = tempfile::Builder::new()
            .prefix("dalo-upgrade-")
            .tempdir()
            .expect("tempdir should be created");
        // The released binary canonicalized the paths it recorded, and the
        // current one compares against canonical paths too, so the replacement
        // root has to be canonical as well (on macOS `/var` is a symlink).
        let root = temp
            .path()
            .canonicalize()
            .expect("temporary root should canonicalize");

        for directory in ["store", "target", "instructions"] {
            copy_tree(&source.join(directory), &root.join(directory));
        }
        for repo in &fixture.repos {
            copy_tree(&source.join(&repo.bare), &root.join(&repo.path));
        }
        restore_git_directories(&root, &fixture.git_dir_placeholder);
        rewrite_recorded_root(&root, &fixture.origin_root, &root);
        for link in &fixture.owned_links {
            std::os::unix::fs::symlink(
                root.join(&link.target),
                root.join("target").join(&link.name),
            )
            .expect("owned link should be recreated");
        }

        Self {
            _temp: temp,
            root,
            fixture,
        }
    }

    fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// Run a command against the restored store and parse its JSON report.
    fn json(&self, arguments: &[&str]) -> serde_json::Value {
        let output = dalo_command()
            .arg("--store")
            .arg(self.store())
            .args(arguments)
            .output()
            .expect("dalo should run");
        assert!(
            output.status.success(),
            "{}: `dalo {}` failed with {:?}\nstderr: {}",
            self.fixture.version,
            arguments.join(" "),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{}: `dalo {}` produced unparsable JSON: {error}\n{}",
                self.fixture.version,
                arguments.join(" "),
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    /// No sync operation may remove a link the fixture already owned.
    fn assert_no_link_removed(&self, report: &serde_json::Value, label: &str) {
        let owned = self
            .fixture
            .owned_links
            .iter()
            .map(|link| self.root.join("target").join(&link.name))
            .collect::<BTreeSet<_>>();
        for operation in report["operations"]
            .as_array()
            .expect("sync operations should be an array")
        {
            let kind = operation["kind"]
                .as_str()
                .expect("operation kind should be a string");
            let path = PathBuf::from(
                operation["link_path"]
                    .as_str()
                    .expect("operation link path should be a string"),
            );
            assert!(
                !(kind.contains("remove") && owned.contains(&path)),
                "{}: `dalo {label}` would remove owned link `{}`",
                self.fixture.version,
                path.display()
            );
        }
        let planned = report["operations"]
            .as_array()
            .expect("sync operations should be an array")
            .iter()
            .map(|operation| {
                PathBuf::from(
                    operation["link_path"]
                        .as_str()
                        .expect("operation link path should be a string"),
                )
            })
            .collect::<BTreeSet<_>>();
        assert!(
            owned.is_subset(&planned),
            "{}: `dalo {label}` dropped an owned link from its plan; planned {planned:?}",
            self.fixture.version
        );
    }

    fn persisted_version(&self, file: &str, field: &str) -> Option<u32> {
        dalo::store::persisted_schema_version(&self.store().join(file), field)
    }
}

/// Description of one fixture, written when the fixture was captured.
#[derive(Debug, Deserialize)]
struct Fixture {
    version: String,
    origin_root: String,
    git_dir_placeholder: String,
    repos: Vec<Repo>,
    owned_links: Vec<OwnedLink>,
    recorded: Recorded,
}

#[derive(Debug, Deserialize)]
struct Repo {
    bare: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct OwnedLink {
    name: String,
    target: String,
}

#[derive(Debug, Deserialize)]
struct Recorded {
    active_skills: Vec<String>,
    pending_approval_skills: Vec<String>,
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stores")
}

fn source_refs(value: &serde_json::Value) -> Vec<String> {
    let mut refs = value
        .as_array()
        .expect("skill list should be an array")
        .iter()
        .map(|skill| {
            skill["source_ref"]
                .as_str()
                .expect("source ref should be a string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    refs.sort();
    refs
}

fn findings_with_severity(report: &serde_json::Value, severity: &str) -> Vec<serde_json::Value> {
    report["findings"]
        .as_array()
        .expect("doctor findings should be an array")
        .iter()
        .filter(|finding| finding["severity"] == severity)
        .cloned()
        .collect()
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("fixture directory should be created");
    for entry in std::fs::read_dir(source).expect("fixture directory should be readable") {
        let entry = entry.expect("fixture entry should be readable");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("fixture entry type should be readable")
            .is_dir()
        {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("fixture file should be copied");
        }
    }
}

/// Nested Git directories cannot be committed as `.git`, so the fixture carries
/// them renamed. Put them back before any Git command sees the store.
fn restore_git_directories(root: &Path, placeholder: &str) {
    for entry in std::fs::read_dir(root).expect("restored directory should be readable") {
        let entry = entry.expect("restored entry should be readable");
        if !entry
            .file_type()
            .expect("restored entry type should be readable")
            .is_dir()
        {
            continue;
        }
        if entry.file_name() == placeholder {
            std::fs::rename(entry.path(), entry.path().with_file_name(".git"))
                .expect("git directory should be restored");
        } else {
            restore_git_directories(&entry.path(), placeholder);
        }
    }
}

/// Replace the root the fixture was captured under with the temporary one.
///
/// A released binary persists absolute paths in `config.toml`, `state.toml`,
/// `lock.toml`, and in each checkout's `remote.origin.url`, so one textual
/// substitution over every UTF-8 file relocates all of them at once. Git object
/// files are binary and never contain the path, so they are left alone.
fn rewrite_recorded_root(directory: &Path, origin_root: &str, root: &Path) {
    for entry in std::fs::read_dir(directory).expect("restored directory should be readable") {
        let entry = entry.expect("restored entry should be readable");
        let path = entry.path();
        if entry
            .file_type()
            .expect("restored entry type should be readable")
            .is_dir()
        {
            rewrite_recorded_root(&path, origin_root, root);
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.contains(origin_root) {
            continue;
        }
        let rewritten = content.replace(
            origin_root,
            root.to_str().expect("temporary root should be utf8"),
        );
        std::fs::write(&path, rewritten).expect("relocated file should be written");
    }
}
