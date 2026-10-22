//! Offline installation of the assistant bundled with this Dalo binary.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::source::SourceKind;
use crate::store::{self, StorePaths};

/// Read-only state of the assistant supplied by the running executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantState {
    /// No store exists yet.
    MissingStore,
    /// The store has no bundled assistant.
    Missing,
    /// The complete installed bundle matches this binary.
    Current,
    /// An unmodified bundle differs from this binary's bundle.
    UpdateAvailable,
    /// Another installation occupies a discovered assistant slot.
    External,
    /// Inspection or safe replacement is blocked.
    Blocked,
}

/// Assistant freshness and delivery, without network access or mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssistantStatusReport {
    /// Selected store.
    pub store: PathBuf,
    /// Bundled local skill slot.
    pub skill_path: PathBuf,
    /// Bundle version supplied by this executable, not an upstream release check.
    pub version: String,
    /// Recorded installed version, when verified.
    pub installed_version: Option<String>,
    /// Bundle installation state.
    pub state: AssistantState,
    /// Explanation of an external or blocked installation.
    pub reason: Option<String>,
    /// Discovered assistant entries not owned as links to this local bundle.
    pub external_paths: Vec<PathBuf>,
    /// Enabled targets that do not currently read the local bundle through an owned link.
    pub undelivered_targets: Vec<String>,
    /// Verified owned assistant links, including companion-only agent folders.
    pub delivered_paths: Vec<PathBuf>,
    /// Suggested explicit next operation, if needed.
    pub next_command: Option<String>,
}

/// Inspect the local bundle and bounded known agent folders, preserving errors
/// as a blocked report instead of treating unreadable content as absent.
pub fn status(paths: &StorePaths) -> AssistantStatusReport {
    let mut report = AssistantStatusReport {
        store: paths.root.clone(),
        skill_path: paths.local_skills_dir.join("dalo"),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        installed_version: None,
        state: AssistantState::MissingStore,
        reason: None,
        external_paths: Vec::new(),
        undelivered_targets: Vec::new(),
        delivered_paths: Vec::new(),
        next_command: None,
    };
    if let Err(error) = inspect(paths, &mut report) {
        report.state = AssistantState::Blocked;
        report.reason = Some(error.to_string());
        report.next_command = None;
    }
    report
}

fn inspect(paths: &StorePaths, report: &mut AssistantStatusReport) -> DaloResult<()> {
    let initialized = entry_exists(&paths.config_file)?;
    if !initialized && entry_exists(&paths.root)? {
        require_real_directory(&paths.root)?;
        if fs::read_dir(&paths.root)?.next().is_some() {
            return Err(blocked(
                "the store has content but no config; preserve and inspect it before setup",
            ));
        }
    }
    let state = if initialized {
        let state = store::read_state(paths)?;
        let installed = install(paths, true)?;
        report.installed_version = installed.previous_version;
        report.state = match installed.action {
            AssistantInstallAction::Install => AssistantState::Missing,
            AssistantInstallAction::Update => AssistantState::UpdateAvailable,
            AssistantInstallAction::Existing => AssistantState::Current,
        };
        state
    } else {
        store::StateFile::default()
    };
    let detection = crate::target::detect_targets(&paths.root)?;
    let mut candidates: BTreeSet<PathBuf> = detection
        .targets
        .into_iter()
        .filter_map(|target| target.path.map(|path| path.join("dalo")))
        .collect();
    // Older Codex installs and project-local skills can be managed by another
    // installer even when Dalo has not linked that location.
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        candidates.insert(PathBuf::from(home).join("skills/dalo"));
    } else if let Some(home) = std::env::var_os("HOME") {
        candidates.insert(PathBuf::from(home).join(".codex/skills/dalo"));
    }
    for variable in ["CLAUDE_CONFIG_DIR", "OPENCODE_CONFIG_DIR"] {
        if let Some(directory) = std::env::var_os(variable) {
            candidates.insert(PathBuf::from(directory).join("skills/dalo"));
        }
    }
    if let Some(directory) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.insert(PathBuf::from(directory).join("opencode/skills/dalo"));
    }
    let cwd = std::env::current_dir()?;
    for relative in [
        ".agents/skills/dalo",
        ".claude/skills/dalo",
        ".codex/skills/dalo",
    ] {
        candidates.insert(cwd.join(relative));
    }
    let companion_links = read_companion_links(paths)?;
    let owns_bundle_link = |path: &Path| {
        let Ok(destination) = fs::read_link(path) else {
            return false;
        };
        let destination = if destination.is_absolute() {
            destination
        } else {
            path.parent().unwrap_or(path).join(destination)
        };
        let points_to_bundle =
            store::comparable_path(&destination) == store::comparable_path(&report.skill_path);
        points_to_bundle
            && (companion_links.links.contains(&comparable_entry(path))
                || state.owned_skills.iter().any(|owned| {
                    comparable_entry(&owned.link_path) == comparable_entry(path)
                        && store::comparable_path(&owned.store_path)
                            == store::comparable_path(&report.skill_path)
                        && store::comparable_path(&destination)
                            == store::comparable_path(&report.skill_path)
                }))
    };
    candidates.extend(companion_links.links.iter().cloned());
    for target in state.targets.iter().filter(|target| target.enabled) {
        let path = target.path.join("dalo");
        if !owns_bundle_link(&path) {
            report.undelivered_targets.push(target.id.clone());
        }
        candidates.insert(path);
    }
    for candidate in candidates {
        if owns_bundle_link(&candidate) {
            report.delivered_paths.push(comparable_entry(&candidate));
        } else if entry_exists(&candidate)? {
            report.external_paths.push(candidate);
        }
    }
    report.delivered_paths.sort();
    report.delivered_paths.dedup();
    if !report.external_paths.is_empty()
        && matches!(
            report.state,
            AssistantState::Missing | AssistantState::MissingStore
        )
    {
        report.state = AssistantState::External;
        report.reason = Some("An assistant already exists outside this store's bundled installation; keep its installer ownership and inspect it before switching.".to_owned());
    }
    let next = match report.state {
        AssistantState::MissingStore => Some("init"),
        AssistantState::Missing | AssistantState::UpdateAvailable => Some("assistant install"),
        AssistantState::Current if !report.undelivered_targets.is_empty() => Some("--dry-run sync"),
        AssistantState::Current
            if report.delivered_paths.is_empty()
                && !state.targets.iter().any(|target| target.enabled) =>
        {
            Some("target detect")
        }
        _ => None,
    };
    report.next_command = next.map(|command| store::dalo_command(&paths.root, command));
    Ok(())
}

fn comparable_entry(path: &Path) -> PathBuf {
    store::comparable_path(path.parent().unwrap_or(path)).join(path.file_name().unwrap_or_default())
}

const RECEIPT: &str = ".dalo-bundle.toml";
const RECEIPT_VERSION: u32 = 1;
const COMPANION_LINKS: &str = "assistant-links.toml";
const NOTICE_MARKER: &str = ".assistant-notice";

/// Whether this store is the default global store under `~/.dalo`.
///
/// Its companion links are the ones other stores see as an assistant installed
/// outside their own bundle, so only this store reports that state unprompted.
pub(crate) fn is_global_store(paths: &StorePaths) -> bool {
    store::expand_user_path(Path::new("~/.dalo"))
        .is_ok_and(|global| store::comparable_path(&global) == store::comparable_path(&paths.root))
}

/// Whether a normal invocation should print a standing blocked or external
/// assistant notice, recording that it did.
///
/// The notice is shown once per initialized store: `.assistant-notice` holds
/// the text last shown, so a different reason is shown again and the same one
/// stays quiet. An uninitialized store cannot remember anything, so a store
/// that does not exist yet stays quiet until `init` creates it, while an
/// existing directory without config keeps reporting why setup is blocked.
/// A marker that is not a regular file is preserved and never followed.
pub(crate) fn take_standing_notice(paths: &StorePaths, notice: &str) -> bool {
    match fs::symlink_metadata(&paths.config_file) {
        Ok(metadata) if metadata.is_file() => {}
        _ => return entry_exists(&paths.root).unwrap_or(true),
    }
    let marker = paths.root.join(NOTICE_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_file() => {
            if fs::read_to_string(&marker).is_ok_and(|shown| shown.trim_end() == notice) {
                return false;
            }
        }
        Ok(_) => return true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return true,
    }
    // Remembering is a convenience; a read-only or busy store still shows the
    // notice and simply shows it again next time.
    let _ = write_notice_marker(&marker, notice);
    true
}

/// Forget a recorded standing notice once the assistant no longer needs one,
/// so a later blocked or external state is reported again.
pub(crate) fn clear_standing_notice(paths: &StorePaths) {
    let marker = paths.root.join(NOTICE_MARKER);
    if fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.is_file()) {
        let _ = fs::remove_file(marker);
    }
}

fn write_notice_marker(marker: &Path, notice: &str) -> DaloResult<()> {
    use std::io::Write;
    let parent = marker.parent().unwrap_or(marker);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(format!("{notice}\n").as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(marker).map_err(|error| error.error)?;
    Ok(())
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompanionLinks {
    schema_version: u32,
    links: BTreeSet<PathBuf>,
}

fn read_companion_links(paths: &StorePaths) -> DaloResult<CompanionLinks> {
    let path = paths.root.join(COMPANION_LINKS);
    if !entry_exists(&path)? {
        return Ok(CompanionLinks {
            schema_version: 1,
            ..Default::default()
        });
    }
    if !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(blocked(
            "assistant-links.toml must be a regular owned receipt",
        ));
    }
    let receipt: CompanionLinks = toml::from_str(&fs::read_to_string(&path)?)
        .map_err(|error| blocked(format!("invalid assistant link receipt: {error}")))?;
    if receipt.schema_version != 1 {
        return Err(blocked(format!(
            "unsupported assistant link receipt schema {}",
            receipt.schema_version
        )));
    }
    Ok(receipt)
}

/// Bootstrap only the packaged companion on ordinary use. Detected folders
/// receive this skill without becoming targets for any other managed content.
///
/// Returns the state before and after setup, so callers can report only what
/// this invocation changed.
pub(crate) fn on_use(
    paths: &StorePaths,
    discover: bool,
) -> DaloResult<(AssistantStatusReport, AssistantStatusReport)> {
    let before = status(paths);
    on_use_from(paths, discover, &before).map(|after| (before, after))
}

fn on_use_from(
    paths: &StorePaths,
    discover: bool,
    before: &AssistantStatusReport,
) -> DaloResult<AssistantStatusReport> {
    if matches!(
        before.state,
        AssistantState::External | AssistantState::Blocked
    ) {
        return Ok(before.clone());
    }
    if before.state == AssistantState::MissingStore {
        fs::create_dir_all(&paths.root)?;
    }
    let _lock = store::StoreLock::acquire(paths)?;
    if !entry_exists(&paths.config_file)? {
        require_real_directory(&paths.root)?;
        for entry in fs::read_dir(&paths.root)? {
            if entry?.path() != paths.lock_guard_file {
                return Err(blocked(
                    "the uninitialized store has unexpected content; inspect it before first-use setup",
                ));
            }
        }
        store::init_store(paths.root.clone(), false)?;
    }
    let mut report = prepare_automatic(paths);
    if report.state != AssistantState::Current {
        return Ok(report);
    }
    // Validate persisted state before creating links. No source or approval
    // snapshot is rebuilt or acknowledged by companion-only setup.
    store::read_user_lock(paths)?;
    let mut receipt = read_companion_links(paths)?;
    let targets = crate::target::detect_targets(&paths.root)?;
    let mut conflicts = Vec::new();
    for target in targets
        .targets
        .into_iter()
        .filter(|target| target.exists && (discover || target.linked))
    {
        let Some(directory) = target.path else {
            continue;
        };
        if !fs::metadata(&directory)?.is_dir() {
            continue;
        }
        let link = fs::canonicalize(directory)?.join("dalo");
        if report
            .delivered_paths
            .iter()
            .any(|path| comparable_entry(path) == link)
        {
            continue;
        }
        if entry_exists(&link)? {
            conflicts.push(link);
            continue;
        }
        std::os::unix::fs::symlink(&report.skill_path, &link)?;
        receipt.links.insert(link.clone());
        if let Err(error) = store::write_toml_atomic(&paths.root.join(COMPANION_LINKS), &receipt) {
            // Only undo the link this attempt created, and only while it still
            // points to this bundle. Preserve anything that replaced it.
            if fs::read_link(&link).is_ok_and(|target| target == report.skill_path) {
                fs::remove_file(&link)?;
            }
            return Err(error);
        }
        report.delivered_paths.push(link);
    }
    report = status(paths);
    if !conflicts.is_empty() {
        report.state = AssistantState::Blocked;
        report.next_command = None;
        report.reason = Some(format!(
            "existing assistant entries were preserved: {}",
            conflicts
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(report)
}
const FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../skills/dalo/SKILL.md")),
    (
        "agents/openai.yaml",
        include_str!("../skills/dalo/agents/openai.yaml"),
    ),
    (
        "references/inventory.md",
        include_str!("../skills/dalo/references/inventory.md"),
    ),
    (
        "references/setup.md",
        include_str!("../skills/dalo/references/setup.md"),
    ),
    (
        "references/migration.md",
        include_str!("../skills/dalo/references/migration.md"),
    ),
    (
        "references/maintenance.md",
        include_str!("../skills/dalo/references/maintenance.md"),
    ),
];

/// Outcome of preparing the assistant in the local source.
#[derive(Debug, Serialize)]
pub struct AssistantInstallReport {
    /// Store used by this operation.
    pub store: PathBuf,
    /// Skill directory in the local source.
    pub skill_path: PathBuf,
    /// Dalo release that supplies this bundle.
    pub version: String,
    /// Previously installed bundle release, if any.
    pub previous_version: Option<String>,
    /// Intended change; `dry_run` determines whether it was applied.
    pub action: AssistantInstallAction,
    /// Whether the command only inspected and planned.
    pub dry_run: bool,
    /// Preview target changes before syncing the whole store.
    pub next_command: String,
}

/// Change to the bundled assistant's local source directory.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantInstallAction {
    /// Install into an unoccupied local slot.
    Install,
    /// Replace a verified, unmodified earlier bundle.
    Update,
    /// The binary's bundle is already present.
    Existing,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BundleReceipt {
    schema_version: u32,
    dalo_version: String,
    entries: BTreeMap<PathBuf, BundleEntry>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct BundleEntry {
    kind: String,
    executable: bool,
    sha256: String,
}

/// Install the embedded assistant into the initialized store's local source.
///
/// This does not link targets, fetch sources, commit local work, or run sync.
/// Existing bundles are replaced only when their complete tree still matches
/// their versioned receipt. Interrupted transactions are retained for recovery.
pub fn install(paths: &StorePaths, dry_run: bool) -> DaloResult<AssistantInstallReport> {
    install_bundle(paths, dry_run, env!("CARGO_PKG_VERSION"), FILES)
}

fn install_bundle(
    paths: &StorePaths,
    dry_run: bool,
    version: &str,
    files: &[(&str, &str)],
) -> DaloResult<AssistantInstallReport> {
    store::read_config(paths)?;
    let _lock = if dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(paths)?)
    };
    install_bundle_using(paths, dry_run, version, files, publish)
}

/// Whether ordinary setup should prepare the bundled skill. Explicit assistant
/// installation remains available when automatic setup is disabled.
pub(crate) fn automatic_setup_enabled() -> bool {
    !std::env::var("DALO_ASSISTANT_CHECK").is_ok_and(|value| value.eq_ignore_ascii_case("never"))
}

/// Prepare the bundle during a write operation, under the caller's store lock.
/// A blocked assistant is advisory: the requested store operation can still
/// succeed, and its report explains why conversational setup needs attention.
pub(crate) fn prepare_automatic(paths: &StorePaths) -> AssistantStatusReport {
    let mut report = status(paths);
    if matches!(
        report.state,
        AssistantState::Current
            | AssistantState::External
            | AssistantState::Blocked
            | AssistantState::MissingStore
    ) {
        return report;
    }
    let previous = match store::read_user_lock(paths) {
        Ok(lock) => lock,
        Err(error) => {
            report.state = AssistantState::Blocked;
            report.next_command = None;
            report.reason = Some(error.to_string());
            return report;
        }
    };
    if report.state == AssistantState::Missing {
        let owns_other_assistant = previous
            .active_skills
            .iter()
            .chain(&previous.pending_approval_skills)
            .chain(&previous.unlinked_skills)
            .any(|skill| skill.slot_name == "dalo" && skill.source_ref != "local:dalo");
        let other_source = (|| -> DaloResult<bool> {
            let config = store::read_config(paths)?;
            if !config
                .sources
                .iter()
                .any(|source| source.enabled && source.id != "local")
            {
                return Ok(false);
            }
            let approvals = store::read_approvals(paths)?;
            let live = crate::resolver::resolve_from_config(&config, approvals.approvals);
            if live.scans.iter().any(|scan| scan.inventory.is_none()) {
                return Err(blocked(
                    "a source could not be inspected for an existing dalo skill; resolve its inventory before automatic assistant setup",
                ));
            }
            Ok(live
                .resolution
                .active_skills
                .iter()
                .chain(&live.resolution.pending_approval_skills)
                .chain(
                    live.resolution
                        .unlinked_skills
                        .iter()
                        .map(|skill| &skill.skill),
                )
                .any(|skill| skill.slot_name == "dalo" && skill.source_id != "local"))
        })();
        match other_source {
            Ok(other) if owns_other_assistant || other => {
                report.state = AssistantState::External;
                report.next_command = None;
                report.reason = Some("another source already supplies a dalo skill; keep its source ownership and inspect it before switching".to_owned());
                return report;
            }
            Err(error) => {
                report.state = AssistantState::Blocked;
                report.next_command = None;
                report.reason = Some(error.to_string());
                return report;
            }
            Ok(_) => {}
        }
    }
    if matches!(
        report.state,
        AssistantState::Missing | AssistantState::UpdateAvailable
    ) && let Err(error) =
        install_bundle_using(paths, false, env!("CARGO_PKG_VERSION"), FILES, publish)
    {
        report.state = AssistantState::Blocked;
        report.next_command = None;
        report.reason = Some(error.to_string());
        return report;
    }
    status(paths)
}

/// Complete only assistant delivery to a newly linked target. The caller holds
/// the store lock; no source refresh, general sync, or approval changes occur.
pub(crate) fn setup_target(paths: &StorePaths, target: &str) -> AssistantStatusReport {
    let mut report = prepare_automatic(paths);
    if report.state != AssistantState::Current {
        return report;
    }
    // Even a symlink to this bundle can belong to a different installer. Do
    // not adopt it, or replace a different Dalo-managed skill in this slot.
    let result = store::read_state(paths).map(|state| {
        state.targets.iter().any(|entry| {
            entry.id == target
                && report.external_paths.iter().any(|external| {
                    comparable_entry(external) == comparable_entry(&entry.path.join("dalo"))
                })
        })
    });
    match result {
        Ok(true) => {
            report.state = AssistantState::Blocked;
            report.next_command = None;
            report.reason = Some("the target's dalo slot belongs to another installation; preserve it and inspect `dalo assistant status` before switching".to_owned());
            return report;
        }
        Err(error) => {
            report.state = AssistantState::Blocked;
            report.next_command = None;
            report.reason = Some(error.to_string());
            return report;
        }
        Ok(false) => {}
    }
    match crate::materialize::materialize_assistant(paths, target) {
        Ok(operations) => {
            report = status(paths);
            if let Some(blocked) = operations.iter().find(|operation| {
                operation.status == crate::materialize::MaterializeOperationStatus::Blocked
                    || operation.kind == crate::materialize::MaterializeOperationKind::Keep
            }) {
                report.state = AssistantState::Blocked;
                report.next_command = None;
                report.reason = blocked.reason.clone();
            }
        }
        Err(error) => {
            report.state = AssistantState::Blocked;
            report.next_command = None;
            report.reason = Some(error.to_string());
        }
    }
    report
}

fn install_bundle_using(
    paths: &StorePaths,
    dry_run: bool,
    version: &str,
    files: &[(&str, &str)],
    mut publish: impl FnMut(&Path, &Path) -> DaloResult<()>,
) -> DaloResult<AssistantInstallReport> {
    store::read_config(paths)?;
    let config = store::read_config(paths)?;
    if !config.sources.iter().any(|source| {
        source.id == "local"
            && source.kind == SourceKind::Local
            && source.enabled
            && source.namespace.is_none()
            && source.subpath.is_none()
            && store::comparable_path(&source.path) == store::comparable_path(&paths.local_dir)
    }) {
        return Err(blocked(
            "the assistant requires the enabled, unnamespaced local source at this store's local directory",
        ));
    }
    require_real_directory(&paths.local_dir)?;
    require_real_directory(&paths.local_skills_dir)?;
    let skill_path = paths.local_skills_dir.join("dalo");
    let transaction = paths.root.join(".assistant-installing");
    if entry_exists(&transaction)? {
        return Err(blocked(format!(
            "an interrupted assistant installation exists at `{}`; inspect its `previous` and `new` directories and recover them before retrying",
            transaction.display()
        )));
    }
    let previous = read_existing(&skill_path)?;
    let desired = bundle_receipt(version, files);
    let action = match &previous {
        Some(receipt) if *receipt == desired => AssistantInstallAction::Existing,
        Some(_) => AssistantInstallAction::Update,
        None => AssistantInstallAction::Install,
    };
    let report = AssistantInstallReport {
        store: paths.root.clone(),
        skill_path: skill_path.clone(),
        version: version.to_owned(),
        previous_version: previous
            .as_ref()
            .map(|receipt| receipt.dalo_version.clone()),
        action,
        dry_run,
        next_command: store::dalo_command(&paths.root, "--dry-run sync"),
    };
    if dry_run || report.action == AssistantInstallAction::Existing {
        return Ok(report);
    }

    // Keep staging outside source discovery. A killed process leaves recovery
    // data here; a subsequent invocation refuses to guess which copy to keep.
    fs::create_dir(&transaction)?;
    let staging = transaction.join("new");
    let prepare = (|| -> DaloResult<()> {
        fs::create_dir(&staging)?;
        for (relative, content) in files {
            let path = staging.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content)?;
        }
        store::write_toml_atomic(&staging.join(RECEIPT), &desired)?;
        if read_existing(&skill_path)? != previous {
            return Err(blocked(
                "the assistant changed during installation; retry after reviewing those changes",
            ));
        }
        Ok(())
    })();
    if let Err(error) = prepare {
        fs::remove_dir_all(&transaction)?;
        return Err(error);
    }

    let backup = transaction.join("previous");
    if previous.is_some() {
        publish(&skill_path, &backup)?;
    }
    let replacement = (|| {
        if previous.is_some() && read_existing(&backup)? != previous {
            return Err(blocked(
                "the previous assistant changed before replacement; preserving those changes",
            ));
        }
        publish(&staging, &skill_path)
    })();
    if let Err(error) = replacement {
        if previous.is_some()
            && let Err(recovery_error) = publish(&backup, &skill_path)
        {
            return Err(blocked(format!(
                "assistant installation failed: {error}; recovery could not restore `{}`: {recovery_error}; the previous bundle remains at `{}`",
                skill_path.display(),
                backup.display()
            )));
        }
        fs::remove_dir_all(&transaction)?;
        return Err(error);
    }
    fs::remove_dir_all(&transaction)?;
    Ok(report)
}

fn publish(from: &Path, to: &Path) -> DaloResult<()> {
    // An existence check followed by rename would still overwrite an empty
    // directory created concurrently. The OS must enforce no replacement.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(std::io::Error::from)?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(blocked(format!(
            "safe assistant publication from `{}` to `{}` requires Linux or macOS",
            from.display(),
            to.display()
        )))
    }
}

fn read_existing(path: &Path) -> DaloResult<Option<BundleReceipt>> {
    if !entry_exists(path)? {
        return Ok(None);
    }
    require_real_directory(path)?;
    let receipt_path = path.join(RECEIPT);
    if !fs::symlink_metadata(&receipt_path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(blocked(format!(
            "`{}` is not a recorded bundled assistant; preserve it and resolve the local dalo slot before installing",
            path.display()
        )));
    }
    let receipt: BundleReceipt =
        toml::from_str(&fs::read_to_string(&receipt_path)?).map_err(|error| {
            DaloError::FileParse {
                path: receipt_path.clone(),
                reason: error.to_string(),
            }
        })?;
    if receipt.schema_version != RECEIPT_VERSION {
        return Err(blocked(format!(
            "unsupported assistant receipt schema {} at `{}`; use a compatible Dalo version",
            receipt.schema_version,
            receipt_path.display()
        )));
    }
    let mut entries = BTreeMap::new();
    snapshot(path, path, &mut entries)?;
    if entries != receipt.entries {
        return Err(blocked(format!(
            "the bundled assistant at `{}` has local changes; preserve or move your complete customized skill before installing another bundle",
            path.display()
        )));
    }
    Ok(Some(receipt))
}

fn snapshot(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<PathBuf, BundleEntry>,
) -> DaloResult<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("entries remain under the snapshot root");
        if relative == Path::new(RECEIPT) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        let value = if metadata.is_dir() {
            snapshot(root, &path, entries)?;
            directory_entry()
        } else if metadata.is_file() {
            file_entry(
                &fs::read(&path)?,
                metadata.permissions().mode() & 0o111 != 0,
            )
        } else {
            return Err(blocked(format!(
                "the assistant contains a symlink or special entry at `{}`; preserve local changes before updating",
                path.display()
            )));
        };
        entries.insert(relative.to_path_buf(), value);
    }
    Ok(())
}

fn bundle_receipt(version: &str, files: &[(&str, &str)]) -> BundleReceipt {
    let mut entries = BTreeMap::new();
    for (name, content) in files {
        let path = Path::new(name);
        entries.insert(path.to_path_buf(), file_entry(content.as_bytes(), false));
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|path| !path.as_os_str().is_empty())
        {
            entries.insert(parent.to_path_buf(), directory_entry());
        }
    }
    BundleReceipt {
        schema_version: RECEIPT_VERSION,
        dalo_version: version.to_owned(),
        entries,
    }
}

fn directory_entry() -> BundleEntry {
    BundleEntry {
        kind: "directory".to_owned(),
        executable: false,
        sha256: String::new(),
    }
}

fn file_entry(content: &[u8], executable: bool) -> BundleEntry {
    let sha256 = Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    BundleEntry {
        kind: "file".to_owned(),
        executable,
        sha256,
    }
}

fn require_real_directory(path: &Path) -> DaloResult<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(blocked(format!(
            "`{}` must be a real directory; no symlink or file will be replaced",
            path.display()
        )));
    }
    Ok(())
}

fn entry_exists(path: &Path) -> DaloResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn blocked(reason: impl Into<String>) -> DaloError {
    DaloError::StateError {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn store() -> (tempfile::TempDir, StorePaths) {
        let temporary = tempfile::tempdir().unwrap();
        let paths = StorePaths::new(temporary.path().join("store"));
        store::init_store(paths.root.clone(), false).unwrap();
        (temporary, paths)
    }

    #[test]
    fn updates_only_verified_bundles_and_does_not_rewrite_an_identical_install() {
        let (_temporary, paths) = store();
        let old = [
            ("SKILL.md", "# Original"),
            ("references/old.md", "Old reference"),
        ];
        let first = install_bundle(&paths, false, "0.1.0", &old).unwrap();
        assert_eq!(first.action, AssistantInstallAction::Install);
        let old_receipt = fs::read(first.skill_path.join(RECEIPT)).unwrap();
        let preview = install(&paths, true).unwrap();
        assert_eq!(preview.action, AssistantInstallAction::Update);
        assert_eq!(
            fs::read(first.skill_path.join(RECEIPT)).unwrap(),
            old_receipt
        );
        let updated = install(&paths, false).unwrap();
        assert_eq!(updated.previous_version.as_deref(), Some("0.1.0"));
        assert!(!updated.skill_path.join("references/old.md").exists());
        for (relative, contents) in FILES {
            assert_eq!(
                fs::read_to_string(updated.skill_path.join(relative)).unwrap(),
                *contents
            );
        }
        let modified = fs::metadata(updated.skill_path.join(RECEIPT))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            install(&paths, false).unwrap().action,
            AssistantInstallAction::Existing
        );
        assert_eq!(
            fs::metadata(updated.skill_path.join(RECEIPT))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        assert!(!paths.root.join(".assistant-installing").exists());
    }

    #[test]
    fn preserves_modified_files_added_directories_permissions_and_symlinks() {
        for change in ["file", "extra", "directory", "executable", "symlink"] {
            let (_temporary, paths) = store();
            let directory = install(&paths, false).unwrap().skill_path;
            match change {
                "file" => fs::write(directory.join("SKILL.md"), "My changes").unwrap(),
                "extra" => fs::write(directory.join("notes.md"), "My notes").unwrap(),
                "directory" => fs::create_dir(directory.join("empty")).unwrap(),
                "executable" => fs::set_permissions(
                    directory.join("SKILL.md"),
                    fs::Permissions::from_mode(0o755),
                )
                .unwrap(),
                "symlink" => symlink("SKILL.md", directory.join("alias")).unwrap(),
                _ => unreachable!(),
            }
            for dry_run in [true, false] {
                assert!(install(&paths, dry_run).is_err(), "{change}");
                assert!(!paths.root.join(".assistant-installing").exists());
            }
            assert!(directory.exists());
        }
    }

    #[test]
    fn refuses_unmanaged_and_redirected_slots_and_local_source_paths() {
        for slot in ["directory", "symlink", "broken", "parent"] {
            let (temporary, paths) = store();
            let outside = temporary.path().join("outside");
            fs::create_dir(&outside).unwrap();
            fs::write(outside.join("SKILL.md"), "Keep this").unwrap();
            let destination = paths.local_skills_dir.join("dalo");
            match slot {
                "directory" => fs::create_dir(&destination).unwrap(),
                "symlink" => symlink(&outside, &destination).unwrap(),
                "broken" => symlink(outside.join("missing"), &destination).unwrap(),
                "parent" => {
                    fs::remove_dir(&paths.local_skills_dir).unwrap();
                    symlink(&outside, &paths.local_skills_dir).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(install(&paths, false).is_err(), "{slot}");
            assert_eq!(
                fs::read_to_string(outside.join("SKILL.md")).unwrap(),
                "Keep this"
            );
        }
    }

    #[test]
    fn preserves_unknown_receipts_and_interrupted_installations() {
        let (_temporary, paths) = store();
        let directory = install(&paths, false).unwrap().skill_path;
        let receipt = directory.join(RECEIPT);
        let original = fs::read_to_string(&receipt).unwrap();
        let future = original.replace("schema_version = 1", "schema_version = 99");
        fs::write(&receipt, &future).unwrap();
        assert!(install(&paths, false).is_err());
        assert_eq!(fs::read_to_string(&receipt).unwrap(), future);
        fs::write(&receipt, original).unwrap();
        let recovery = paths.root.join(".assistant-installing");
        fs::create_dir(&recovery).unwrap();
        fs::rename(&directory, recovery.join("previous")).unwrap();
        assert!(install(&paths, false).is_err());
        assert!(recovery.join("previous/SKILL.md").exists());
        assert!(!directory.exists());
    }

    #[test]
    fn publishing_never_replaces_even_an_empty_concurrent_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let staged = temporary.path().join("staged");
        let occupied = temporary.path().join("occupied");
        fs::create_dir(&staged).unwrap();
        fs::write(staged.join("payload"), "New").unwrap();
        fs::create_dir(&occupied).unwrap();
        assert!(publish(&staged, &occupied).is_err());
        assert!(staged.join("payload").exists());
        assert_eq!(fs::read_dir(&occupied).unwrap().count(), 0);
    }

    #[test]
    fn failed_replacement_restores_the_complete_previous_bundle() {
        let (_temporary, paths) = store();
        let old = [
            ("SKILL.md", "# Original"),
            ("references/old.md", "Keep this"),
        ];
        let installed = install_bundle(&paths, false, "0.1.0", &old).unwrap();
        let receipt = fs::read(installed.skill_path.join(RECEIPT)).unwrap();
        let failure = install_bundle_using(&paths, false, "0.2.0", FILES, |from, to| {
            if from.file_name().unwrap() == "new" {
                Err(std::io::Error::other("injected publication failure").into())
            } else {
                publish(from, to)
            }
        });
        assert!(
            failure
                .unwrap_err()
                .to_string()
                .contains("injected publication failure")
        );
        assert_eq!(
            fs::read(installed.skill_path.join(RECEIPT)).unwrap(),
            receipt
        );
        assert_eq!(
            fs::read_to_string(installed.skill_path.join("references/old.md")).unwrap(),
            "Keep this"
        );
        assert!(!paths.root.join(".assistant-installing").exists());
        assert_eq!(
            install_bundle(&paths, false, "0.1.0", &old).unwrap().action,
            AssistantInstallAction::Existing
        );
    }

    #[test]
    fn concurrent_occupation_preserves_both_recovery_and_foreign_content() {
        let (_temporary, paths) = store();
        let old = [("SKILL.md", "# Original")];
        let installed = install_bundle(&paths, false, "0.1.0", &old).unwrap();
        let failure = install_bundle_using(&paths, false, "0.2.0", FILES, |from, to| {
            if from.file_name().unwrap() == "new" {
                fs::create_dir(to)?;
                fs::write(to.join("SKILL.md"), "# Concurrent custom skill")?;
            }
            publish(from, to)
        });
        assert!(
            failure
                .unwrap_err()
                .to_string()
                .contains("the previous bundle remains at")
        );
        assert_eq!(
            fs::read_to_string(installed.skill_path.join("SKILL.md")).unwrap(),
            "# Concurrent custom skill"
        );
        let transaction = paths.root.join(".assistant-installing");
        assert_eq!(
            fs::read_to_string(transaction.join("previous/SKILL.md")).unwrap(),
            "# Original"
        );
        assert!(transaction.join("new/SKILL.md").is_file());
        assert!(
            install(&paths, false)
                .unwrap_err()
                .to_string()
                .contains("interrupted assistant installation")
        );
    }
}
