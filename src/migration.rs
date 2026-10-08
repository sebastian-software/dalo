//! Verified handover of project installations created by the skills CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{DaloError, DaloResult};
use crate::project::{Manifest, Project, ProjectSource};
use crate::{catalog, git, inventory, project, source};

const LOCK: &str = "skills-lock.json";
const BACKUP: &str = ".dalo-migration-backup";
const TARGETS: &[(&str, &str)] = &[
    ("codex", ".agents/skills"),
    ("claude", ".claude/skills"),
    ("opencode", ".opencode/skills"),
    ("hermes", ".hermes/skills"),
];

#[derive(Deserialize)]
struct SkillsLock {
    version: u32,
    skills: BTreeMap<String, LockEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockEntry {
    source: String,
    source_type: String,
    source_url: Option<String>,
    #[serde(rename = "ref")]
    revision: Option<String>,
    skill_path: Option<String>,
    computed_hash: String,
}

/// One verified or blocked skill in a migration preview.
#[derive(Debug, Serialize)]
pub struct MigrationSkill {
    /// Name recorded by skills.sh.
    pub name: String,
    /// Exact verified commit, not the foreign folder hash.
    pub commit: Option<String>,
    /// Project-relative entries that would be preserved in the backup.
    pub paths: Vec<PathBuf>,
    /// Why this entry cannot be migrated without a separate decision.
    pub blocked: Option<String>,
}

/// Migration preview/result. No approval is imported from the foreign lock.
#[derive(Debug, Serialize)]
pub struct MigrationReport {
    /// Project root.
    pub project: PathBuf,
    /// Whether the handover was applied.
    pub applied: bool,
    /// Whether all entries can be handed over.
    pub ready: bool,
    /// Agent targets inferred from the existing supported folders.
    pub targets: Vec<String>,
    /// Verification results in lockfile order.
    pub skills: Vec<MigrationSkill>,
    /// Backup location; created only during an explicit apply.
    pub backup: PathBuf,
}

struct Move {
    relative: PathBuf,
    hash: String,
    link: Option<PathBuf>,
}

fn invalid(reason: impl Into<String>) -> DaloError {
    DaloError::InvalidArgument {
        reason: reason.into(),
    }
}

fn exists(path: &Path) -> DaloResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// Foreign nested links can refer to content outside the verified snapshot.
fn regular_tree(path: &Path) -> DaloResult<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            regular_tree(&entry.path())?;
        } else if !kind.is_file() {
            return Err(invalid(
                "nested symlinks or special files need manual migration",
            ));
        }
    }
    Ok(())
}

/// Find the nearest foreign project lock without crossing a Git boundary.
pub fn discover(start: &Path) -> DaloResult<PathBuf> {
    let start = fs::canonicalize(start)?;
    for root in start.ancestors() {
        if exists(&root.join(LOCK))? {
            return Ok(root.to_path_buf());
        }
        if exists(&root.join(".git"))? {
            break;
        }
    }
    Err(invalid(
        "no skills-lock.json found in this project; use --project <directory>",
    ))
}

fn no_symlink_ancestors(root: &Path, relative: &Path) -> DaloResult<()> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(invalid("invalid migration path"));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(invalid("migration path has a redirected parent"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn read_lock(root: &Path) -> DaloResult<Vec<u8>> {
    no_symlink_ancestors(root, Path::new(LOCK))?;
    if !fs::metadata(root.join(LOCK))?.is_file() {
        return Err(invalid("skills-lock.json must be a regular file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(root.join(LOCK))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid("skills-lock.json exceeds 1 MiB"));
    }
    Ok(bytes)
}

fn origin(entry: &LockEntry, root: &Path) -> DaloResult<String> {
    if !matches!(
        entry.source_type.as_str(),
        "github" | "git" | "gitlab" | "bitbucket"
    ) {
        return Err(invalid(
            "this source type needs manual migration; only Git repositories are supported",
        ));
    }
    let url = if let Some(url) = &entry.source_url {
        url.clone()
    } else if entry.source_type == "github" {
        let parts: Vec<_> = entry.source.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || *p == "."
                    || *p == ".."
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return Err(invalid("GitHub source must identify owner/repository"));
        }
        format!("https://github.com/{}.git", entry.source)
    } else {
        entry.source.clone()
    };
    git::validate_remote_url(&url)?;
    Ok(source::resolve_source_location(&url, root))
}

fn inspect_paths(root: &Path, name: &str) -> DaloResult<Vec<(String, Move)>> {
    if !source::is_valid_source_id(name) {
        return Err(invalid("unsafe installed skill name"));
    }
    let mut result = Vec::new();
    for (target, folder) in TARGETS {
        no_symlink_ancestors(root, Path::new(folder))?;
        let relative = Path::new(folder).join(name);
        let path = root.join(&relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let link = metadata
            .file_type()
            .is_symlink()
            .then(|| fs::read_link(&path))
            .transpose()?;
        let resolved = fs::canonicalize(&path)?;
        if !resolved.starts_with(root) || !resolved.is_dir() {
            return Err(invalid(
                "installed skill is not a directory inside this project",
            ));
        }
        // Only in-project aliases of a recorded canonical skills slot can be
        // handed over. Otherwise moving a target could strand another source.
        if link.is_some()
            && !TARGETS
                .iter()
                .any(|(_, folder)| root.join(folder).join(name) == resolved)
        {
            return Err(invalid(
                "skill symlink does not point to a supported project skill slot",
            ));
        }
        regular_tree(&resolved)?;
        result.push((
            (*target).into(),
            Move {
                relative,
                hash: catalog::hash_directory(&resolved)?,
                link,
            },
        ));
    }
    if result.is_empty() {
        return Err(invalid(
            "no installed copy found in supported project agent folders",
        ));
    }
    Ok(result)
}

/// Preview by default; explicitly apply only a fully verified, unchanged plan.
/// Remote reads occur during verification, including dry-run. No skill code runs.
pub fn migrate(root: &Path, apply: bool) -> DaloResult<MigrationReport> {
    let project = Project::new(root)?;
    let root = &project.root;
    for path in [project::MANIFEST, ".dalo", BACKUP] {
        if exists(&root.join(path))? {
            return Err(invalid(format!(
                "{path} already exists; migration will not replace it"
            )));
        }
    }
    let original_lock = read_lock(root)?;
    let lock: SkillsLock = serde_json::from_slice(&original_lock)
        .map_err(|e| invalid(format!("invalid skills-lock.json: {e}")))?;
    if lock.version != 1 || lock.skills.is_empty() {
        return Err(invalid(
            "expected a non-empty skills-lock.json with version 1",
        ));
    }
    let temporary = tempfile::tempdir()?;
    let mut checkouts: BTreeMap<(String, Option<String>), (PathBuf, String)> = BTreeMap::new();
    let mut sources: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut targets = BTreeSet::new();
    let mut moves = Vec::new();
    let mut report = MigrationReport {
        project: root.clone(),
        applied: false,
        ready: true,
        skills: vec![],
        targets: vec![],
        backup: root.join(BACKUP),
    };
    for (name, entry) in &lock.skills {
        let mut item = MigrationSkill {
            name: name.clone(),
            commit: None,
            paths: vec![],
            blocked: None,
        };
        let result = (|| -> DaloResult<()> {
            if entry.computed_hash.len() != 64
                || !entry.computed_hash.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid(
                    "invalid skills.sh content hash; it is not a Git commit ID",
                ));
            }
            let installed = inspect_paths(root, name)?;
            item.paths = installed.iter().map(|(_, m)| m.relative.clone()).collect();
            let url = origin(entry, root)?;
            if let Some(revision) = &entry.revision {
                git::validate_manifest_revision(revision)?;
            }
            let key = (url.clone(), entry.revision.clone());
            if !checkouts.contains_key(&key) {
                let checkout = temporary.path().join(format!("source-{}", checkouts.len()));
                source::clone_source_checkout(&url, &checkout)?;
                let commit = if let Some(revision) = &entry.revision {
                    git::resolve_manifest_revision(&checkout, revision)?
                } else {
                    git::rev_parse_head(&checkout)?
                };
                git::checkout_detached(&checkout, &commit)?;
                checkouts.insert(key.clone(), (checkout, commit));
            }
            let (checkout, commit) = &checkouts[&key];
            let skill_path = entry
                .skill_path
                .as_ref()
                .map(|path| {
                    let path = source::validate_source_subpath(Path::new(path))?;
                    if path.file_name().is_none_or(|name| name != "SKILL.md") {
                        return Err(invalid("skillPath must name SKILL.md"));
                    }
                    Ok(path.parent().unwrap_or(Path::new("")).to_path_buf())
                })
                .transpose()?;
            let inventory = inventory::scan_source("migration", checkout)?;
            let candidates: Vec<_> = inventory
                .skills
                .iter()
                .filter(|s| {
                    s.slot_name == *name
                        && skill_path
                            .as_ref()
                            .is_none_or(|p| s.path == checkout.join(p))
                })
                .collect();
            if candidates.len() != 1 {
                return Err(invalid("source skill identity is missing or ambiguous"));
            }
            let skill = candidates[0];
            let hash = if skill.path == *checkout {
                catalog::hash_source_root_directory(&skill.path)?
            } else {
                catalog::hash_directory(&skill.path)?
            };
            if installed.iter().any(|(_, m)| m.hash != hash) {
                return Err(invalid(
                    "installed content differs from the candidate commit; preserve local edits or choose an explicit update",
                ));
            }
            sources.entry((url, commit.clone())).or_default().push(
                skill.id.clone().unwrap_or_else(|| {
                    let relative = skill
                        .path
                        .strip_prefix(checkout)
                        .expect("scanned source path");
                    if relative.as_os_str().is_empty() {
                        skill.slot_name.clone()
                    } else {
                        relative.to_string_lossy().into_owned()
                    }
                }),
            );
            item.commit = Some(commit.clone());
            for (target, movement) in installed {
                targets.insert(target);
                moves.push(movement);
            }
            Ok(())
        })();
        if let Err(error) = result {
            item.blocked = Some(error.to_string());
            report.ready = false;
        }
        report.skills.push(item);
    }
    report.targets = targets.iter().cloned().collect();
    if apply && report.ready {
        let manifest = Manifest {
            schema_version: 1,
            approval: None,
            targets: targets.into_iter().collect(),
            sources: sources
                .into_iter()
                .enumerate()
                .map(|(i, ((url, commit), skills))| ProjectSource {
                    id: format!("imported-{}", i + 1),
                    url,
                    commit,
                    skills,
                })
                .collect(),
        };
        apply_plan(root, &original_lock, &moves, &manifest, &report)?;
        report.applied = true;
    }
    Ok(report)
}

fn apply_plan(
    root: &Path,
    lock: &[u8],
    moves: &[Move],
    manifest: &Manifest,
    report: &MigrationReport,
) -> DaloResult<()> {
    if read_lock(root)? != lock {
        return Err(invalid(
            "skills-lock.json changed during migration; rerun the preview",
        ));
    }
    // Recheck all copies and link identities before the first move.
    for movement in moves {
        no_symlink_ancestors(root, movement.relative.parent().expect("slot parent"))?;
        let path = root.join(&movement.relative);
        let metadata = fs::symlink_metadata(&path)?;
        let link = metadata
            .file_type()
            .is_symlink()
            .then(|| fs::read_link(&path))
            .transpose()?;
        let canonical = fs::canonicalize(&path)?;
        if link != movement.link
            || !canonical.starts_with(root)
            || catalog::hash_directory(&canonical)? != movement.hash
        {
            return Err(invalid(
                "installed skill changed during migration; no handover performed",
            ));
        }
    }
    let mut staged = tempfile::NamedTempFile::new_in(root)?;
    staged.write_all(toml::to_string_pretty(manifest)?.as_bytes())?;
    staged.as_file().sync_all()?;
    let backup = root.join(BACKUP);
    fs::create_dir(&backup)?;
    // Persist recovery information before moving anything. An interrupted run
    // leaves the original relative layout here, never an automatic overwrite.
    let mut recovery = fs::File::create(backup.join("migration.json"))?;
    recovery.write_all(&serde_json::to_vec_pretty(report)?)?;
    recovery.sync_all()?;
    let mut moved = Vec::new();
    let result = (|| -> DaloResult<()> {
        for relative in moves
            .iter()
            .map(|m| m.relative.clone())
            .chain([PathBuf::from(LOCK)])
        {
            let destination = backup.join(&relative);
            fs::create_dir_all(destination.parent().expect("backup parent"))?;
            fs::rename(root.join(&relative), &destination)?;
            moved.push(relative);
        }
        staged
            .persist_noclobber(root.join(project::MANIFEST))
            .map_err(|e| e.error)?;
        Ok(())
    })();
    if let Err(error) = result {
        for relative in moved.iter().rev() {
            if exists(&root.join(relative))? {
                return Err(invalid(format!(
                    "migration interrupted; originals remain in {}; restore only into empty paths",
                    backup.display()
                )));
            }
            fs::rename(backup.join(relative), root.join(relative))?;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_handover_restores_already_moved_content() {
        let temp = tempfile::tempdir().unwrap();
        let canonical_root = fs::canonicalize(temp.path()).unwrap();
        let root = canonical_root.as_path();
        fs::create_dir_all(root.join(".agents/skills/review")).unwrap();
        fs::write(root.join(".agents/skills/review/SKILL.md"), "original").unwrap();
        fs::write(root.join(LOCK), "original lock").unwrap();
        let relative = PathBuf::from(".agents/skills/review");
        let movement = || Move {
            relative: relative.clone(),
            hash: catalog::hash_directory(&root.join(&relative)).unwrap(),
            link: None,
        };
        // Both entries pass preflight, but the second rename fails after the
        // first move. This exercises restoration after a partial handover.
        let moves = vec![movement(), movement()];
        let manifest = Manifest {
            schema_version: 1,
            approval: None,
            targets: vec!["codex".into()],
            sources: vec![],
        };
        let report = MigrationReport {
            project: root.into(),
            applied: false,
            ready: true,
            targets: vec!["codex".into()],
            skills: vec![],
            backup: root.join(BACKUP),
        };
        assert!(apply_plan(root, b"original lock", &moves, &manifest, &report).is_err());
        assert_eq!(
            fs::read_to_string(root.join(relative).join("SKILL.md")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(root.join(LOCK)).unwrap(),
            "original lock"
        );
        assert!(!root.join(project::MANIFEST).exists());
        assert!(root.join(BACKUP).join("migration.json").exists());
    }
}
