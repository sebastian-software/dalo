//! Reference store generator for the published performance envelope.
//!
//! This is test support code, not a shipped command. It builds a complete
//! store — Git sources, catalogs, selections, approvals, linked targets, and an
//! instruction pack — entirely from local `git init` repositories, so both the
//! CI smoke test and the `--ignored` measurement run observe the same shape at
//! two different sizes. Nothing here touches the network or the real `HOME`.

#![allow(dead_code)]

use super::TestEnvironment;
use assert_cmd::Command;
use dalo::store::{self, ApprovalRecord, ApprovalsFile, StorePaths};
use dalo::{catalog, instructions, source, target};
use std::path::{Path, PathBuf};

/// Target IDs used by a scenario, each pointed at a temporary directory.
///
/// `generic` always requires an explicit path; `codex` and `claude` accept one,
/// which keeps every materialization inside the scenario's own temporary root.
const TARGET_IDS: [&str; 3] = ["generic", "codex", "claude"];

/// The ID of the instruction pack every scenario activates.
pub const INSTRUCTION_PACK_ID: &str = "team-style";

/// The size of a generated store.
///
/// [`ScenarioSpec::reference`] is the published envelope's scenario;
/// [`ScenarioSpec::smoke`] is the small variant CI can afford to build on every
/// run. Both produce the same kinds of work, so a hot-path regression shows up
/// in the small one too.
#[derive(Clone, Copy, Debug)]
pub struct ScenarioSpec {
    pub team_sources: usize,
    pub skills_per_team_source: usize,
    pub catalogs: usize,
    pub skills_per_catalog: usize,
    pub selected_per_catalog: usize,
    pub targets: usize,
}

impl ScenarioSpec {
    /// The reference scenario: 10 sources, 200 skills, 3 targets.
    ///
    /// Six tracking team sources of 20 skills each contribute 120 active
    /// skills; four pinned catalogs offer 20 skills each, of which 5 are
    /// selected and source-approved. That matches the scale the v0.4 audit set
    /// as the design target.
    #[must_use]
    pub const fn reference() -> Self {
        Self {
            team_sources: 6,
            skills_per_team_source: 20,
            catalogs: 4,
            skills_per_catalog: 20,
            selected_per_catalog: 5,
            targets: 3,
        }
    }

    /// The CI-sized variant of the reference scenario.
    ///
    /// Same mix of source kinds, selections, approvals, targets, and one
    /// instruction pack — roughly a twentieth of the skills, so building it
    /// stays in the low seconds even in a debug build.
    #[must_use]
    pub const fn smoke() -> Self {
        Self {
            team_sources: 3,
            skills_per_team_source: 4,
            catalogs: 2,
            skills_per_catalog: 4,
            selected_per_catalog: 2,
            targets: 2,
        }
    }

    /// The smallest store that still exercises the same code path.
    ///
    /// The smoke test divides by this baseline to cancel out how fast the
    /// machine running the test happens to be.
    #[must_use]
    pub const fn baseline() -> Self {
        Self {
            team_sources: 1,
            skills_per_team_source: 1,
            catalogs: 0,
            skills_per_catalog: 0,
            selected_per_catalog: 0,
            targets: 1,
        }
    }

    #[must_use]
    pub const fn sources(&self) -> usize {
        self.team_sources + self.catalogs
    }

    /// Every skill Dalo can see, including catalog offers that stay inactive.
    #[must_use]
    pub const fn skills(&self) -> usize {
        self.team_sources * self.skills_per_team_source + self.catalogs * self.skills_per_catalog
    }

    /// Skills that resolve to an active, materialized slot.
    #[must_use]
    pub const fn active_skills(&self) -> usize {
        self.team_sources * self.skills_per_team_source + self.catalogs * self.selected_per_catalog
    }

    /// One source approval per catalog.
    #[must_use]
    pub const fn approvals(&self) -> usize {
        self.catalogs
    }
}

/// A fully built store plus the isolated environment its commands run in.
///
/// Dropping the scenario removes the temporary root, the store, every source
/// repository, and every target directory.
pub struct Scenario {
    _root: tempfile::TempDir,
    environment: TestEnvironment,
    pub spec: ScenarioSpec,
    pub store: PathBuf,
    pub targets: Vec<PathBuf>,
    pub instruction_file: PathBuf,
    pub scratch: PathBuf,
}

impl Scenario {
    /// Builds a store matching `spec` from local Git repositories.
    ///
    /// Setup goes through the library rather than the CLI so that measurement
    /// runs spend their time on the commands being measured, not on building
    /// the fixture. The resulting store is byte-identical in shape to one a
    /// user would produce with `init`, `target link`, `source add`,
    /// `source add-catalog`, `source select`, `approve source`, and
    /// `instructions enable`.
    #[must_use]
    pub fn build(spec: ScenarioSpec) -> Self {
        assert!(
            spec.targets <= TARGET_IDS.len(),
            "a scenario links at most {} targets",
            TARGET_IDS.len()
        );
        assert!(
            spec.selected_per_catalog <= spec.skills_per_catalog,
            "a catalog cannot select more skills than it offers"
        );

        let environment = TestEnvironment::create();
        let root = tempfile::Builder::new()
            .prefix("dalo-scenario-")
            .tempdir()
            .expect("scenario root should be created");
        let store = root.path().join("store");
        store::init_store(store.clone(), false).expect("scenario store should initialize");
        let paths = StorePaths::new(store.clone());

        let targets = link_targets(&store, root.path(), spec.targets);
        add_team_sources(&paths, root.path(), spec);
        add_catalogs(&paths, root.path(), spec);
        let instruction_file = enable_instruction_pack(&paths, &store, root.path());

        let scratch = root.path().join("scratch");
        std::fs::create_dir_all(&scratch).expect("scenario scratch should be created");

        Self {
            _root: root,
            environment,
            spec,
            store,
            targets,
            instruction_file,
            scratch,
        }
    }

    /// A Dalo command already bound to this scenario's store and environment.
    #[must_use]
    pub fn dalo(&self) -> Command {
        let mut command = self.environment.command();
        command.arg("--store").arg(&self.store);
        command
    }

    #[must_use]
    pub fn environment(&self) -> &TestEnvironment {
        &self.environment
    }
}

fn link_targets(store: &Path, root: &Path, count: usize) -> Vec<PathBuf> {
    TARGET_IDS
        .iter()
        .take(count)
        .map(|id| {
            let path = root.join("targets").join(id);
            std::fs::create_dir_all(&path).expect("scenario target should be created");
            target::link_target(store, id, Some(&path), false).expect("target should link");
            path
        })
        .collect()
}

fn add_team_sources(paths: &StorePaths, root: &Path, spec: ScenarioSpec) {
    for index in 0..spec.team_sources {
        let id = format!("team{index}");
        let repo = root.join("repos").join(&id);
        write_skill_repository(&repo, &id, spec.skills_per_team_source);
        source::add_team_source(paths, &id, repository_url(&repo).as_str(), None, false)
            .expect("team source should be added");
    }
}

fn add_catalogs(paths: &StorePaths, root: &Path, spec: ScenarioSpec) {
    for index in 0..spec.catalogs {
        let id = format!("catalog{index}");
        let repo = root.join("repos").join(&id);
        write_skill_repository(&repo, &id, spec.skills_per_catalog);
        catalog::add_catalog_source(paths, &id, repository_url(&repo).as_str(), None, false)
            .expect("catalog source should be added");
        let selection: Vec<String> = (0..spec.selected_per_catalog)
            .map(|skill| slot_name(&id, skill))
            .collect();
        catalog::select_skills(paths, &id, &selection, false, false)
            .expect("catalog skills should be selected");
    }
    approve_sources(paths, spec.catalogs);
}

/// Grants one source approval per catalog so its selections become active.
fn approve_sources(paths: &StorePaths, catalogs: usize) {
    let mut approvals: ApprovalsFile =
        store::read_approvals(paths).expect("approvals should be readable");
    for index in 0..catalogs {
        approvals.approvals.push(ApprovalRecord {
            scope: "source".to_owned(),
            value: format!("catalog{index}"),
            granted_at_unix: None,
        });
    }
    store::write_approvals(paths, &approvals).expect("source approvals should be written");
}

fn enable_instruction_pack(paths: &StorePaths, store: &Path, root: &Path) -> PathBuf {
    let pack = store
        .join("local/instructions")
        .join(format!("{INSTRUCTION_PACK_ID}.md"));
    std::fs::create_dir_all(pack.parent().expect("pack has a parent"))
        .expect("instruction directory should exist");
    std::fs::write(
        &pack,
        "version: 1.0.0\n\
         topics: review, formatting\n\
         \n\
         # Team Style\n\
         \n\
         Keep review comments short and describe the behavior that changed.\n",
    )
    .expect("instruction pack should be written");

    let instruction_file = root.join("AGENTS.md");
    std::fs::write(&instruction_file, "# Scenario project\n\n")
        .expect("instruction file should be written");
    instructions::enable_pack(paths, INSTRUCTION_PACK_ID, &instruction_file, false)
        .expect("instruction pack should be enabled");
    instruction_file
}

fn write_skill_repository(repo: &Path, source_id: &str, skills: usize) {
    for index in 0..skills {
        let slot = slot_name(source_id, index);
        let directory = repo.join("skills").join(&slot);
        std::fs::create_dir_all(&directory).expect("scenario skill directory should be created");
        std::fs::write(directory.join("SKILL.md"), skill_body(&slot))
            .expect("scenario skill should be written");
    }
    std::fs::create_dir_all(repo).expect("scenario repository should be created");
    std::fs::write(
        repo.join("README.md"),
        format!("# {source_id}\n\nGenerated reference scenario repository.\n"),
    )
    .expect("scenario readme should be written");
    super::init_git_repo(repo);
}

fn slot_name(source_id: &str, index: usize) -> String {
    format!("{source_id}-skill-{index:03}")
}

fn skill_body(slot: &str) -> String {
    format!(
        "---\n\
         id: {slot}\n\
         name: {slot}\n\
         description: Reference scenario skill used to measure the performance envelope.\n\
         tags:\n\
         \x20 - reference\n\
         ---\n\
         \n\
         # {slot}\n\
         \n\
         Describe the change, name the behavior it affects, and keep the summary short.\n"
    )
}

fn repository_url(repo: &Path) -> String {
    repo.to_str()
        .expect("scenario repository path should be utf8")
        .to_owned()
}
