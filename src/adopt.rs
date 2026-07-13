//! Adoption and minimal repair operations for unmanaged target skills.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::store::{self, OwnedSkillState, ProtectedSkillState, StorePaths};

const SKILL_FILE: &str = "SKILL.md";

/// Unmanaged skill discovered in a linked target directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnmanagedSkill {
    /// Stable CLI-facing ID. Usually the slot name, or the full path when ambiguous.
    pub id: String,
    /// Target slot name.
    pub slot_name: String,
    /// Skill directory path.
    pub path: PathBuf,
    /// Logical target IDs using the materialization directory.
    pub target_ids: Vec<String>,
    /// Whether the skill is protected from replacement.
    pub protected: bool,
}

/// Adopt command report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdoptReport {
    /// Adopted slot name.
    pub slot_name: String,
    /// Original unmanaged skill path.
    pub source_path: PathBuf,
    /// Local source destination.
    pub local_path: PathBuf,
    /// Copy status.
    pub copy: AdoptCopyStatus,
    /// Optional replacement status.
    pub replacement: AdoptReplacementStatus,
    /// Action needed when the original unmanaged skill remains at the same slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

/// Copy status for adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptCopyStatus {
    /// Copy would run in dry-run mode.
    Planned,
    /// Skill was copied into the local source.
    Copied,
    /// A local copy already existed and was reused (second step of a two-step adopt).
    Existing,
}

impl AdoptCopyStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Copied => "copied",
            Self::Existing => "existing",
        }
    }
}

/// Replacement status for adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptReplacementStatus {
    /// Replacement would run in dry-run mode.
    Planned,
    /// Original unmanaged folder was replaced with an owned symlink.
    Replaced,
    /// Replacement was not requested.
    Skipped,
    /// Original skill is protected and was left untouched.
    Protected,
}

impl AdoptReplacementStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Replaced => "replaced",
            Self::Skipped => "skipped",
            Self::Protected => "protected",
        }
    }
}

/// Resolve list report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolveListReport {
    /// Unmanaged skills in linked targets.
    pub unmanaged_skills: Vec<UnmanagedSkill>,
    /// Non-fatal target directory scan warnings.
    pub target_warnings: Vec<TargetScanWarning>,
    /// Recorded owned symlinks.
    pub owned_skills: Vec<OwnedSkillSummary>,
}

/// Unmanaged target scan result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnmanagedSkillScan {
    /// Unmanaged skills in linked targets.
    pub unmanaged_skills: Vec<UnmanagedSkill>,
    /// Non-fatal target directory scan warnings.
    pub warnings: Vec<TargetScanWarning>,
}

/// Non-fatal target scan warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetScanWarning {
    /// Warning code.
    pub code: TargetScanWarningCode,
    /// Path related to the warning.
    pub path: PathBuf,
    /// Human-readable message.
    pub message: String,
}

/// Target scan warning code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetScanWarningCode {
    /// A materialization directory or child entry could not be read.
    UnreadableTargetDir,
}

impl TargetScanWarningCode {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnreadableTargetDir => "unreadable_target_dir",
        }
    }
}

/// Recorded owned symlink summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedSkillSummary {
    /// Repair ID accepted by `resolve remove-owned`.
    pub id: String,
    /// Target slot name.
    pub slot_name: String,
    /// Link path.
    pub link_path: PathBuf,
    /// Store path.
    pub store_path: PathBuf,
}

/// Keep/protect report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeepReport {
    /// Protected unmanaged skill.
    pub skill: UnmanagedSkill,
    /// Whether the protection already existed.
    pub existing: bool,
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
    /// Warning when a managed local skill still targets the protected slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Remove-owned report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoveOwnedReport {
    /// Removed or repaired record ID.
    pub id: String,
    /// Link path.
    pub link_path: PathBuf,
    /// Status.
    pub status: RemoveOwnedStatus,
}

/// Remove-owned status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoveOwnedStatus {
    /// Operation would run in dry-run mode.
    Planned,
    /// Owned symlink was removed and the state record was dropped.
    Removed,
    /// State record was dropped because the symlink was already absent.
    DroppedMissing,
    /// A real entry exists at the recorded path, so nothing was removed.
    BlockedRealEntry,
    /// A different symlink occupies the path, so only the stale state record was dropped.
    DroppedForeignSymlink,
}

impl RemoveOwnedStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Removed => "removed",
            Self::DroppedMissing => "dropped_missing",
            Self::BlockedRealEntry => "blocked_real_entry",
            Self::DroppedForeignSymlink => "dropped_foreign_symlink",
        }
    }
}

/// Discover unmanaged skills in configured target directories.
pub fn discover_unmanaged_skills(paths: &StorePaths) -> DaloResult<Vec<UnmanagedSkill>> {
    Ok(discover_unmanaged_skill_scan(paths)?.unmanaged_skills)
}

/// Discover unmanaged skills and non-fatal target scan warnings.
pub fn discover_unmanaged_skill_scan(paths: &StorePaths) -> DaloResult<UnmanagedSkillScan> {
    let state = store::read_state(paths)?;
    let owned_paths = state
        .owned_skills
        .iter()
        .map(|skill| skill.link_path.clone())
        .collect::<BTreeSet<_>>();
    let protected_paths = state
        .protected_skills
        .iter()
        .map(|skill| skill.path.clone())
        .collect::<BTreeSet<_>>();
    let mut found = Vec::new();
    let mut warnings = Vec::new();

    for dir in &state.materialization_dirs {
        if !dir.path.exists() {
            continue;
        }

        let entries = match fs::read_dir(&dir.path) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(unreadable_target_warning(&dir.path, error));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(unreadable_target_warning(&dir.path, error));
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(unreadable_target_warning(&path, error));
                    continue;
                }
            };
            if owned_paths.contains(&path) || file_type.is_symlink() {
                continue;
            }
            if !path.join(SKILL_FILE).is_file() {
                continue;
            }

            let slot_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let protected = is_local_marker(&slot_name) || protected_paths.contains(&path);
            found.push(UnmanagedSkill {
                id: String::new(),
                slot_name,
                path,
                target_ids: dir.logical_targets.clone(),
                protected,
            });
        }
    }

    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(UnmanagedSkillScan {
        unmanaged_skills: assign_unmanaged_ids(found),
        warnings,
    })
}

/// Adopt an unmanaged skill into the local source.
pub fn adopt_skill(
    paths: &StorePaths,
    selector: &str,
    replace_original: bool,
    dry_run: bool,
) -> DaloResult<AdoptReport> {
    let skill = find_unmanaged_skill(paths, selector)?;
    let local_path = paths.local_skills_dir.join(&skill.slot_name);

    let copy = if local_path.exists() {
        // A local copy already exists. A plain `adopt` must never clobber it.
        if !replace_original {
            return Err(DaloError::AdoptionDestinationExists { path: local_path });
        }
        // Replacement may be the second step of a two-step flow ONLY if the
        // existing copy has the same content as the skill being adopted. If it is
        // an unrelated, pre-existing local skill, replacing the target would delete
        // the unmanaged content and link to foreign content — refuse instead.
        if !directories_match(&skill.path, &local_path)? {
            return Err(DaloError::AdoptionDestinationExists { path: local_path });
        }
        AdoptCopyStatus::Existing
    } else if dry_run {
        AdoptCopyStatus::Planned
    } else {
        copy_dir(&skill.path, &local_path)?;
        AdoptCopyStatus::Copied
    };

    let replacement = if replace_original {
        replace_with_owned_symlink(paths, &skill, &local_path, dry_run)?
    } else {
        AdoptReplacementStatus::Skipped
    };

    Ok(AdoptReport {
        next_step: (!replace_original).then(|| {
            format!(
                "the original remains at this slot; run `dalo adopt {} --replace` or remove it before the next sync",
                skill.id
            )
        }),
        slot_name: skill.slot_name,
        source_path: skill.path,
        local_path,
        copy,
        replacement,
    })
}

/// List minimal resolve items.
pub fn list_resolve_items(paths: &StorePaths) -> DaloResult<ResolveListReport> {
    let state = store::read_state(paths)?;
    let mut owned_skills = state
        .owned_skills
        .iter()
        .map(|skill| OwnedSkillSummary {
            id: owned_id(skill),
            slot_name: skill.slot_name.clone(),
            link_path: skill.link_path.clone(),
            store_path: skill.store_path.clone(),
        })
        .collect::<Vec<_>>();
    owned_skills.sort_by(|left, right| left.id.cmp(&right.id));

    let scan = discover_unmanaged_skill_scan(paths)?;
    Ok(ResolveListReport {
        unmanaged_skills: scan.unmanaged_skills,
        target_warnings: scan.warnings,
        owned_skills,
    })
}

/// Mark an unmanaged skill as explicitly protected.
pub fn keep_unmanaged_skill(
    paths: &StorePaths,
    selector: &str,
    dry_run: bool,
) -> DaloResult<KeepReport> {
    let skill = find_unmanaged_skill(paths, selector)?;
    let mut state = store::read_state(paths)?;
    let existing = state
        .protected_skills
        .iter()
        .any(|protected| protected.path == skill.path);
    if !existing && !dry_run {
        state.protected_skills.push(ProtectedSkillState {
            slot_name: skill.slot_name.clone(),
            path: skill.path.clone(),
        });
        state
            .protected_skills
            .sort_by(|left, right| left.path.cmp(&right.path));
        store::write_state(paths, &state)?;
    }

    let local_copy = paths
        .local_skills_dir
        .join(&skill.slot_name)
        .join(SKILL_FILE);
    let warning = local_copy.is_file().then(|| {
        format!(
            "a local managed skill also targets this slot; protection preserves the conflict until you run `dalo resolve adopt {} --replace` or remove the original",
            skill.id
        )
    });

    Ok(KeepReport {
        skill,
        existing,
        dry_run,
        warning,
    })
}

/// Remove a recorded dalo-owned symlink by its unambiguous generated ID.
pub fn remove_owned_skill(
    paths: &StorePaths,
    selector: &str,
    dry_run: bool,
) -> DaloResult<RemoveOwnedReport> {
    let mut state = store::read_state(paths)?;
    let matches = state
        .owned_skills
        .iter()
        .enumerate()
        .filter(|(_, skill)| owned_id(skill) == selector)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = matches.as_slice() else {
        return Err(DaloError::skill_not_found(
            selector,
            state.owned_skills.iter().map(owned_id).collect(),
            "dalo resolve list",
        ));
    };
    let record = state.owned_skills[*index].clone();
    let status = remove_owned_link(&record, dry_run)?;

    if !dry_run {
        state.owned_skills.remove(*index);
        store::write_state(paths, &state)?;
    }

    Ok(RemoveOwnedReport {
        id: owned_id(&record),
        link_path: record.link_path,
        status,
    })
}

fn find_unmanaged_skill(paths: &StorePaths, selector: &str) -> DaloResult<UnmanagedSkill> {
    let skills = discover_unmanaged_skills(paths)?;
    if let Some(skill) = skills
        .iter()
        .find(|skill| skill.id == selector || skill.slot_name == selector)
    {
        return Ok(skill.clone());
    }

    let selector_path = PathBuf::from(selector);
    if selector_path.exists() {
        return unmanaged_from_path(skills, &selector_path);
    }

    Err(DaloError::skill_not_found(
        selector,
        skills.iter().map(|skill| skill.id.clone()).collect(),
        "dalo resolve list",
    ))
}

fn unmanaged_from_path(skills: Vec<UnmanagedSkill>, path: &Path) -> DaloResult<UnmanagedSkill> {
    // Candidate paths come from `entry.path()` and are absolute, but the selector
    // may be relative (`./skills/review`), carry a trailing slash, or route
    // through a symlinked component. Compare on the canonical form so those still
    // match, falling back to the raw path when canonicalization is unavailable.
    // Only skills discovered inside a materialization dir are considered, so the
    // directory boundary that `discover_unmanaged_skills` enforces still holds.
    let target = canonical_or_self(path);
    let known_skills = skills.iter().map(|skill| skill.id.clone()).collect();
    skills
        .into_iter()
        .find(|skill| canonical_or_self(&skill.path) == target)
        .ok_or_else(|| {
            DaloError::skill_not_found(
                path.display().to_string(),
                known_skills,
                "dalo resolve list",
            )
        })
}

/// Canonicalize a path, falling back to the original when it cannot be resolved.
fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn assign_unmanaged_ids(mut skills: Vec<UnmanagedSkill>) -> Vec<UnmanagedSkill> {
    let mut counts = BTreeMap::<String, usize>::new();
    for skill in &skills {
        *counts.entry(skill.slot_name.clone()).or_default() += 1;
    }
    for skill in &mut skills {
        skill.id = if counts.get(&skill.slot_name).copied().unwrap_or_default() > 1 {
            skill.path.display().to_string()
        } else {
            skill.slot_name.clone()
        };
    }
    skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.path.cmp(&right.path))
    });
    skills
}

fn unreadable_target_warning(path: &Path, error: std::io::Error) -> TargetScanWarning {
    TargetScanWarning {
        code: TargetScanWarningCode::UnreadableTargetDir,
        path: path.to_path_buf(),
        message: format!("target path could not be read: {error}"),
    }
}

fn replace_with_owned_symlink(
    paths: &StorePaths,
    skill: &UnmanagedSkill,
    local_path: &Path,
    dry_run: bool,
) -> DaloResult<AdoptReplacementStatus> {
    replace_with_owned_symlink_with_state_writer(
        paths,
        skill,
        local_path,
        dry_run,
        store::write_state,
    )
}

fn replace_with_owned_symlink_with_state_writer<F>(
    paths: &StorePaths,
    skill: &UnmanagedSkill,
    local_path: &Path,
    dry_run: bool,
    write_state: F,
) -> DaloResult<AdoptReplacementStatus>
where
    F: FnOnce(&StorePaths, &store::StateFile) -> DaloResult<()>,
{
    if skill.protected {
        return Ok(AdoptReplacementStatus::Protected);
    }
    if dry_run {
        return Ok(AdoptReplacementStatus::Planned);
    }

    let backup_path = adopting_backup_path(&skill.path)?;
    fs::rename(&skill.path, &backup_path)?;

    if let Err(error) = unix_fs::symlink(local_path, &skill.path) {
        return Err(rollback_replacement(
            &skill.path,
            &backup_path,
            error.into(),
        ));
    }
    let mut state = match store::read_state(paths) {
        Ok(state) => state,
        Err(error) => return Err(rollback_replacement(&skill.path, &backup_path, error)),
    };
    state.owned_skills.push(OwnedSkillState {
        target_id: skill
            .target_ids
            .first()
            .cloned()
            .unwrap_or_else(|| "generic".to_owned()),
        slot_name: skill.slot_name.clone(),
        link_path: skill.path.clone(),
        store_path: local_path.to_path_buf(),
    });
    state.owned_skills.sort_by(|left, right| {
        left.link_path
            .cmp(&right.link_path)
            .then_with(|| left.store_path.cmp(&right.store_path))
    });
    if let Err(error) = write_state(paths, &state) {
        return Err(rollback_replacement(&skill.path, &backup_path, error));
    }
    let _ = fs::remove_dir_all(&backup_path);

    Ok(AdoptReplacementStatus::Replaced)
}

fn adopting_backup_path(path: &Path) -> DaloResult<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        DaloError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("adopt path `{}` has no parent", path.display()),
        ))
    })?;
    let name = path.file_name().ok_or_else(|| {
        DaloError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("adopt path `{}` has no file name", path.display()),
        ))
    })?;
    let candidate = parent.join(format!("{}.dalo-adopting", name.to_string_lossy()));
    if candidate.exists() {
        return Err(DaloError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "adoption backup path `{}` already exists; restore it to `{}` or remove it before retrying",
                candidate.display(),
                path.display()
            ),
        )));
    }
    Ok(candidate)
}

fn rollback_replacement(
    link_path: &Path,
    backup_path: &Path,
    original_error: DaloError,
) -> DaloError {
    match restore_replacement_backup(link_path, backup_path) {
        Ok(()) => original_error,
        Err(restore_error) => DaloError::Io(io::Error::other(format!(
            "{original_error}; also failed to restore original from `{}`: {restore_error}",
            backup_path.display()
        ))),
    }
}

fn restore_replacement_backup(link_path: &Path, backup_path: &Path) -> DaloResult<()> {
    match fs::symlink_metadata(link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::remove_file(link_path)?,
        Ok(_) => {
            return Err(DaloError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("replacement path `{}` is occupied", link_path.display()),
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::rename(backup_path, link_path)?;
    Ok(())
}

fn remove_owned_link(record: &OwnedSkillState, dry_run: bool) -> DaloResult<RemoveOwnedStatus> {
    match fs::symlink_metadata(&record.link_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&record.link_path)? != record.store_path {
                return Ok(RemoveOwnedStatus::DroppedForeignSymlink);
            }
            if dry_run {
                Ok(RemoveOwnedStatus::Planned)
            } else {
                fs::remove_file(&record.link_path)?;
                Ok(RemoveOwnedStatus::Removed)
            }
        }
        Ok(_) => Ok(RemoveOwnedStatus::BlockedRealEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if dry_run {
                Ok(RemoveOwnedStatus::Planned)
            } else {
                Ok(RemoveOwnedStatus::DroppedMissing)
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn copy_dir(source: &Path, destination: &Path) -> DaloResult<()> {
    copy_dir_with(source, destination, |source, destination| {
        fs::copy(source, destination)
    })
}

fn copy_dir_with<F>(source: &Path, destination: &Path, mut copy_file: F) -> DaloResult<()>
where
    F: FnMut(&Path, &Path) -> io::Result<u64>,
{
    let parent = destination.parent().ok_or_else(|| {
        DaloError::Io(io::Error::other(format!(
            "cannot create an adoption copy without a parent for {}",
            destination.display()
        )))
    })?;
    fs::create_dir_all(parent)?;
    let name = destination
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("skill"));
    let temporary = tempfile::Builder::new()
        .prefix(&format!(".{}.dalo-adopting-", name.to_string_lossy()))
        .tempdir_in(parent)?;
    copy_dir_contents(source, temporary.path(), &mut copy_file)?;
    fs::rename(temporary.path(), destination)?;
    Ok(())
}

fn copy_dir_contents<F>(source: &Path, destination: &Path, copy_file: &mut F) -> DaloResult<()>
where
    F: FnMut(&Path, &Path) -> io::Result<u64>,
{
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir(&destination_path)?;
            copy_dir_contents(&source_path, &destination_path, copy_file)?;
        } else if file_type.is_symlink() {
            unix_fs::symlink(fs::read_link(&source_path)?, destination_path)?;
        } else {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

/// Whether two directories have identical content: the same set of relative file
/// paths, each with identical bytes. Used to confirm an existing local destination
/// is the copy a prior `adopt` made — not an unrelated pre-existing local skill —
/// before replacing (and deleting) the unmanaged original. On any doubt it returns
/// `false`, so the caller refuses rather than risk discarding unmanaged content.
fn directories_match(left: &Path, right: &Path) -> DaloResult<bool> {
    let mut left_files = Vec::new();
    let mut right_files = Vec::new();
    collect_relative_files(left, left, &mut left_files)?;
    collect_relative_files(right, right, &mut right_files)?;
    left_files.sort();
    right_files.sort();
    if left_files != right_files {
        return Ok(false);
    }
    for relative in &left_files {
        if fs::read(left.join(relative))? != fs::read(right.join(relative))? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn collect_relative_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> DaloResult<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_relative_files(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn is_local_marker(slot_name: &str) -> bool {
    slot_name.ends_with(".local")
}

fn owned_id(skill: &OwnedSkillState) -> String {
    format!("{}:{}", skill.target_id, skill.slot_name)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use super::*;
    use crate::store::{MaterializationDirState, StateFile, TargetState};

    #[test]
    fn discover_unmanaged_skills_should_ignore_owned_symlinks() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unmanaged = target.join("review");
        let owned = target.join("owned");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::create_dir_all(store_root.join("local/skills/owned"))
            .expect("owned source should be created");
        fs::create_dir_all(&target).expect("target should be created");
        unix_fs::symlink(store_root.join("local/skills/owned"), &owned)
            .expect("owned link should be created");
        write_state(&store_root, &target, &owned);

        let skills = discover_unmanaged_skills(&StorePaths::new(store_root))
            .expect("unmanaged discovery should succeed");

        assert_eq!(skills[0].slot_name, "review");
    }

    #[test]
    fn discover_unmanaged_skill_scan_should_warn_on_unreadable_target_paths() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unreadable = temp_dir.path().join("not-a-dir");
        let unmanaged = target.join("review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::write(&unreadable, "not a directory\n").expect("unreadable path should be written");
        write_state(&store_root, &target, &target.join("unused"));
        let paths = StorePaths::new(store_root);
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.materialization_dirs.push(MaterializationDirState {
            path: unreadable.clone(),
            logical_targets: vec!["other".to_owned()],
        });
        store::write_state(&paths, &state).expect("state should be writable");

        let scan = discover_unmanaged_skill_scan(&paths).expect("scan should be non-fatal");

        assert_eq!(scan.unmanaged_skills[0].slot_name, "review");
        assert_eq!(scan.warnings.len(), 1);
        assert_eq!(
            scan.warnings[0].code,
            TargetScanWarningCode::UnreadableTargetDir
        );
        assert_eq!(scan.warnings[0].path, unreadable);
    }

    #[test]
    fn adopt_skill_should_ignore_unrelated_unreadable_target_paths() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unreadable = temp_dir.path().join("not-a-dir");
        let unmanaged = target.join("review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::write(&unreadable, "not a directory\n").expect("unreadable path should be written");
        write_state(&store_root, &target, &target.join("unused"));
        let paths = StorePaths::new(store_root.clone());
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.materialization_dirs.push(MaterializationDirState {
            path: unreadable,
            logical_targets: vec!["other".to_owned()],
        });
        store::write_state(&paths, &state).expect("state should be writable");

        let report = adopt_skill(&paths, "review", false, false).expect("adopt should succeed");

        assert_eq!(report.slot_name, "review");
        assert!(store_root.join("local/skills/review/SKILL.md").is_file());
    }

    #[test]
    fn adopt_skill_should_copy_before_replacing_original() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unmanaged = target.join("review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        write_state(&store_root, &target, &target.join("unused"));

        let report = adopt_skill(&StorePaths::new(store_root.clone()), "review", true, false)
            .expect("adopt should succeed");

        assert_eq!(report.replacement, AdoptReplacementStatus::Replaced);
        assert!(store_root.join("local/skills/review/SKILL.md").is_file());
        assert!(
            fs::symlink_metadata(unmanaged)
                .expect("replacement should exist")
                .file_type()
                .is_symlink()
        );
        assert!(!target.join("review.dalo-adopting").exists());
    }

    #[test]
    fn copy_dir_should_not_leave_a_partial_destination_when_copying_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source = temp_dir.path().join("source");
        let destination = temp_dir.path().join("local/review");
        fs::create_dir_all(&source).expect("source should be created");
        fs::write(source.join(SKILL_FILE), "# Review\n").expect("skill file should be written");

        let error = copy_dir_with(&source, &destination, |_, _| {
            Err(io::Error::other("injected copy failure"))
        })
        .expect_err("injected copy failure should abort adoption");

        assert!(error.to_string().contains("injected copy failure"));
        assert!(!destination.exists());
    }

    #[test]
    fn replace_with_owned_symlink_should_restore_original_when_state_read_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("missing-store");
        let target = temp_dir.path().join("target");
        let unmanaged = target.join("review");
        let local_path = temp_dir.path().join("local-review");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::create_dir_all(&local_path).expect("local dir should be created");
        fs::write(local_path.join(SKILL_FILE), "# Review\n")
            .expect("local skill should be written");
        let skill = UnmanagedSkill {
            id: "review".to_owned(),
            slot_name: "review".to_owned(),
            path: unmanaged.clone(),
            target_ids: vec!["generic".to_owned()],
            protected: false,
        };

        let error =
            replace_with_owned_symlink(&StorePaths::new(store_root), &skill, &local_path, false)
                .expect_err("missing state should fail replacement");

        assert!(matches!(error, DaloError::StoreNotInitialized { .. }));
        assert!(unmanaged.join(SKILL_FILE).is_file());
        assert!(
            !fs::symlink_metadata(&unmanaged)
                .expect("original should be restored")
                .file_type()
                .is_symlink()
        );
        assert!(!target.join("review.dalo-adopting").exists());
    }

    #[test]
    fn replace_with_owned_symlink_should_restore_original_when_state_write_fails() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unmanaged = target.join("review");
        let local_path = temp_dir.path().join("local-review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        fs::create_dir_all(&local_path).expect("local dir should be created");
        fs::write(local_path.join(SKILL_FILE), "# Review\n")
            .expect("local skill should be written");
        write_state(&store_root, &target, &target.join("unused"));
        let skill = UnmanagedSkill {
            id: "review".to_owned(),
            slot_name: "review".to_owned(),
            path: unmanaged.clone(),
            target_ids: vec!["generic".to_owned()],
            protected: false,
        };
        let result = replace_with_owned_symlink_with_state_writer(
            &StorePaths::new(store_root),
            &skill,
            &local_path,
            false,
            |_, _| Err(DaloError::Io(io::Error::other("state write failed"))),
        );

        let error = result.expect_err("injected state write failure should abort replacement");
        assert!(matches!(error, DaloError::Io(_)));
        assert!(unmanaged.join(SKILL_FILE).is_file());
        assert!(
            !fs::symlink_metadata(&unmanaged)
                .expect("original should be restored")
                .file_type()
                .is_symlink()
        );
        assert!(!target.join("review.dalo-adopting").exists());
    }

    #[test]
    fn rollback_replacement_should_surface_restore_failure() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let link_path = temp_dir.path().join("review");
        let backup_path = temp_dir.path().join("review.dalo-adopting");
        fs::create_dir_all(&link_path).expect("occupied link path should be created");
        fs::create_dir_all(&backup_path).expect("backup path should be created");

        let error = rollback_replacement(
            &link_path,
            &backup_path,
            DaloError::Io(io::Error::other("symlink failed")),
        );

        assert!(
            error
                .to_string()
                .contains("also failed to restore original")
        );
        assert!(backup_path.exists());
    }

    #[test]
    fn adopting_backup_path_should_name_recovery_for_leftover_backup() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let link_path = temp_dir.path().join("review");
        let backup_path = temp_dir.path().join("review.dalo-adopting");
        fs::create_dir_all(&backup_path).expect("backup path should be created");

        let error =
            adopting_backup_path(&link_path).expect_err("existing backup should block retry");

        let message = error.to_string();
        assert!(message.contains("restore it"));
        assert!(message.contains("remove it before retrying"));
        assert!(message.contains(&link_path.display().to_string()));
    }

    #[test]
    fn adopt_skill_should_accept_non_canonical_path_selector() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let unmanaged = target.join("review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&unmanaged).expect("unmanaged dir should be created");
        fs::write(unmanaged.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        write_state(&store_root, &target, &target.join("unused"));
        // A `.`-component path (mirroring a user-supplied `./skills/review`) is a
        // valid handle for the same directory but is not byte-equal to the clean
        // absolute path stored in state; only canonicalization makes them match.
        let selector = target.join(".").join("review");

        let report = adopt_skill(
            &StorePaths::new(store_root),
            &selector.to_string_lossy(),
            false,
            false,
        )
        .expect("non-canonical path selector should resolve");

        assert_eq!(report.slot_name, "review");
    }

    #[test]
    fn adopt_skill_should_reject_path_outside_materialization_dirs() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let outside = temp_dir.path().join("outside/review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&outside).expect("outside dir should be created");
        fs::write(outside.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        write_state(&store_root, &target, &target.join("unused"));

        let error = adopt_skill(
            &StorePaths::new(store_root),
            &outside.to_string_lossy(),
            true,
            false,
        )
        .expect_err("adopt should refuse a path outside materialization dirs");

        assert!(matches!(error, DaloError::SkillNotFound { .. }));
    }

    #[test]
    fn adopt_skill_should_not_remove_path_outside_materialization_dirs() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        let outside = temp_dir.path().join("outside/review");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        fs::create_dir_all(&outside).expect("outside dir should be created");
        fs::write(outside.join(SKILL_FILE), "# Review\n").expect("skill should be written");
        write_state(&store_root, &target, &target.join("unused"));

        let _ = adopt_skill(
            &StorePaths::new(store_root),
            &outside.to_string_lossy(),
            true,
            false,
        );

        assert!(outside.join(SKILL_FILE).is_file());
    }

    fn write_state(store_root: &Path, target: &Path, owned: &Path) {
        fs::create_dir_all(target).expect("target should be created");
        let paths = StorePaths::new(store_root.to_path_buf());
        let state = StateFile {
            schema_version: store::STATE_SCHEMA_VERSION,
            targets: vec![TargetState {
                id: "generic".to_owned(),
                path: target.to_path_buf(),
                canonical_path: target.to_path_buf(),
                enabled: true,
            }],
            materialization_dirs: vec![MaterializationDirState {
                path: target.to_path_buf(),
                logical_targets: vec!["generic".to_owned()],
            }],
            owned_skills: vec![OwnedSkillState {
                target_id: "generic".to_owned(),
                slot_name: "owned".to_owned(),
                link_path: owned.to_path_buf(),
                store_path: store_root.join("local/skills/owned"),
            }],
            protected_skills: Vec::new(),
        };
        store::write_state(&paths, &state).expect("state should be written");
    }
}
