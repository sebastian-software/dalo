//! Adoption and minimal repair operations for unmanaged target skills.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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
}

/// Copy status for adoption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptCopyStatus {
    /// Copy would run in dry-run mode.
    Planned,
    /// Skill was copied into the local source.
    Copied,
}

impl AdoptCopyStatus {
    /// Text label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Copied => "copied",
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
    /// Recorded owned symlinks.
    pub owned_skills: Vec<OwnedSkillSummary>,
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
        }
    }
}

/// Discover unmanaged skills in configured target directories.
pub fn discover_unmanaged_skills(paths: &StorePaths) -> DaloResult<Vec<UnmanagedSkill>> {
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

    for dir in &state.materialization_dirs {
        if !dir.path.is_dir() {
            continue;
        }

        for entry in fs::read_dir(&dir.path)? {
            let entry = entry?;
            let path = entry.path();
            if owned_paths.contains(&path) || entry.file_type()?.is_symlink() {
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

    Ok(assign_unmanaged_ids(found))
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
    if local_path.exists() {
        return Err(DaloError::AdoptionDestinationExists { path: local_path });
    }

    if !dry_run {
        copy_dir(&skill.path, &local_path)?;
    }

    let replacement = if replace_original {
        replace_with_owned_symlink(paths, &skill, &local_path, dry_run)?
    } else {
        AdoptReplacementStatus::Skipped
    };

    Ok(AdoptReport {
        slot_name: skill.slot_name,
        source_path: skill.path,
        local_path,
        copy: if dry_run {
            AdoptCopyStatus::Planned
        } else {
            AdoptCopyStatus::Copied
        },
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

    Ok(ResolveListReport {
        unmanaged_skills: discover_unmanaged_skills(paths)?,
        owned_skills,
    })
}

/// Mark an unmanaged skill as explicitly protected.
pub fn keep_unmanaged_skill(paths: &StorePaths, selector: &str) -> DaloResult<KeepReport> {
    let skill = find_unmanaged_skill(paths, selector)?;
    let mut state = store::read_state(paths)?;
    let existing = state
        .protected_skills
        .iter()
        .any(|protected| protected.path == skill.path);
    if !existing {
        state.protected_skills.push(ProtectedSkillState {
            slot_name: skill.slot_name.clone(),
            path: skill.path.clone(),
        });
        state
            .protected_skills
            .sort_by(|left, right| left.path.cmp(&right.path));
        store::write_state(paths, &state)?;
    }

    Ok(KeepReport { skill, existing })
}

/// Remove a recorded dalo-owned symlink by slot, path, or generated ID.
pub fn remove_owned_skill(
    paths: &StorePaths,
    selector: &str,
    dry_run: bool,
) -> DaloResult<RemoveOwnedReport> {
    let mut state = store::read_state(paths)?;
    let Some(index) = state.owned_skills.iter().position(|skill| {
        skill.slot_name == selector
            || skill.link_path.to_string_lossy() == selector
            || owned_id(skill) == selector
    }) else {
        return Err(DaloError::SkillNotFound {
            skill: selector.to_owned(),
        });
    };
    let record = state.owned_skills[index].clone();
    let status = remove_owned_link(&record.link_path, dry_run)?;

    if !dry_run && status != RemoveOwnedStatus::BlockedRealEntry {
        state.owned_skills.remove(index);
        store::write_state(paths, &state)?;
    }

    Ok(RemoveOwnedReport {
        id: owned_id(&record),
        link_path: record.link_path,
        status,
    })
}

fn find_unmanaged_skill(paths: &StorePaths, selector: &str) -> DaloResult<UnmanagedSkill> {
    let selector_path = PathBuf::from(selector);
    if selector_path.exists() {
        return unmanaged_from_path(paths, &selector_path);
    }

    discover_unmanaged_skills(paths)?
        .into_iter()
        .find(|skill| skill.id == selector || skill.slot_name == selector)
        .ok_or_else(|| DaloError::SkillNotFound {
            skill: selector.to_owned(),
        })
}

fn unmanaged_from_path(paths: &StorePaths, path: &Path) -> DaloResult<UnmanagedSkill> {
    let path = path.to_path_buf();
    discover_unmanaged_skills(paths)?
        .into_iter()
        .find(|skill| skill.path == path)
        .ok_or_else(|| DaloError::SkillNotFound {
            skill: path.display().to_string(),
        })
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

fn replace_with_owned_symlink(
    paths: &StorePaths,
    skill: &UnmanagedSkill,
    local_path: &Path,
    dry_run: bool,
) -> DaloResult<AdoptReplacementStatus> {
    if skill.protected {
        return Ok(AdoptReplacementStatus::Protected);
    }
    if dry_run {
        return Ok(AdoptReplacementStatus::Planned);
    }

    // The skill was already copied into the local source, so its content is safe.
    // Remove the original folder and link it; if linking fails, restore the
    // original from the copy so we never leave a deleted folder with no symlink.
    fs::remove_dir_all(&skill.path)?;
    if let Err(error) = unix_fs::symlink(local_path, &skill.path) {
        let _ = copy_dir(local_path, &skill.path);
        return Err(error.into());
    }
    let mut state = store::read_state(paths)?;
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
    store::write_state(paths, &state)?;

    Ok(AdoptReplacementStatus::Replaced)
}

fn remove_owned_link(path: &Path, dry_run: bool) -> DaloResult<RemoveOwnedStatus> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if dry_run {
                Ok(RemoveOwnedStatus::Planned)
            } else {
                fs::remove_file(path)?;
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
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            unix_fs::symlink(fs::read_link(&source_path)?, destination_path)?;
        } else {
            fs::copy(source_path, destination_path)?;
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
