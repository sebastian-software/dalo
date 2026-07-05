//! Skill inventory scanning.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::DaloResult;

const SKILL_FILE: &str = "SKILL.md";

/// Inventory for one source checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceInventory {
    /// Source ID.
    pub source_id: String,
    /// Scanned skills.
    pub skills: Vec<SkillRecord>,
    /// Non-fatal scan warnings.
    pub warnings: Vec<InventoryWarning>,
}

/// One discovered skill.
///
/// V1.1 (drift detection) will reintroduce content/metadata fingerprints here,
/// computed once and ideally persisted into the user lock so `status`/`doctor`
/// can detect drift without re-hashing every skill directory on each run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillRecord {
    /// Source ID.
    pub source_id: String,
    /// Source-qualified ref, `<source-id>:<slot-name>`.
    pub source_ref: String,
    /// Stable frontmatter ID when present.
    pub id: Option<String>,
    /// Physical install slot name.
    pub slot_name: String,
    /// Skill directory path.
    pub path: PathBuf,
    /// `SKILL.md` path.
    pub skill_file: PathBuf,
    /// Optional description.
    pub description: Option<String>,
    /// Declared dependencies.
    pub requires: Vec<String>,
    /// Declared owners.
    pub owners: Vec<String>,
    /// Declared tags.
    pub tags: Vec<String>,
}

/// Non-fatal inventory warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryWarning {
    /// Warning code.
    pub code: InventoryWarningCode,
    /// Path related to the warning.
    pub path: PathBuf,
    /// Human-readable message.
    pub message: String,
}

/// Inventory warning code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryWarningCode {
    /// Frontmatter exists but could not be parsed.
    MalformedFrontmatter,
    /// Frontmatter name could not be used as a slot name.
    InvalidSlotName,
    /// Multiple skills in the same source have the same slot name.
    DuplicateSlotName,
    /// A skill path could not be read.
    UnreadablePath,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SkillFrontmatter {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    owners: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Scan a source checkout for skills.
pub fn scan_source(source_id: &str, source_root: &Path) -> DaloResult<SourceInventory> {
    let mut warnings = Vec::new();
    let skill_dirs = find_skill_dirs(source_root, &mut warnings)?;
    let mut skills = Vec::new();

    for skill_dir in skill_dirs {
        match scan_skill(source_id, &skill_dir) {
            Ok((skill, mut skill_warnings)) => {
                // `skill` is `None` when the slot name could not be resolved; the
                // skill is dropped while its warning is still collected.
                if let Some(skill) = skill {
                    skills.push(skill);
                }
                warnings.append(&mut skill_warnings);
            }
            Err(error) => warnings.push(InventoryWarning {
                code: InventoryWarningCode::UnreadablePath,
                path: skill_dir,
                message: error.to_string(),
            }),
        }
    }

    skills.sort_by(|left, right| {
        left.slot_name
            .cmp(&right.slot_name)
            .then_with(|| left.source_ref.cmp(&right.source_ref))
            .then_with(|| left.path.cmp(&right.path))
    });
    warnings.extend(duplicate_slot_warnings(source_id, &skills));
    warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| warning_code_name(left.code).cmp(warning_code_name(right.code)))
    });

    Ok(SourceInventory {
        source_id: source_id.to_owned(),
        skills,
        warnings,
    })
}

fn find_skill_dirs(
    source_root: &Path,
    warnings: &mut Vec<InventoryWarning>,
) -> DaloResult<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![source_root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        if dir.file_name().is_some_and(|name| name == ".git") {
            continue;
        }

        let skill_file = dir.join(SKILL_FILE);
        if skill_file.is_file() {
            found.push(dir);
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(InventoryWarning {
                    code: InventoryWarningCode::UnreadablePath,
                    path: dir,
                    message: error.to_string(),
                });
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(InventoryWarning {
                        code: InventoryWarningCode::UnreadablePath,
                        path: dir.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(InventoryWarning {
                        code: InventoryWarningCode::UnreadablePath,
                        path: entry.path(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if file_type.is_dir() {
                pending.push(entry.path());
            }
        }
    }

    found.sort();
    Ok(found)
}

fn scan_skill(
    source_id: &str,
    skill_dir: &Path,
) -> DaloResult<(Option<SkillRecord>, Vec<InventoryWarning>)> {
    let skill_file = skill_dir.join(SKILL_FILE);
    let skill_markdown = fs::read_to_string(&skill_file)?;
    let (frontmatter, mut warnings) = parse_frontmatter(&skill_markdown, &skill_file);
    let folder_name = skill_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_id.to_owned());
    let Some(slot_name) = select_slot_name(&frontmatter, &folder_name, &skill_file, &mut warnings)
    else {
        // Neither the front-matter name nor the folder name is a usable slot
        // name; drop the skill but keep the warning so callers can surface it.
        return Ok((None, warnings));
    };
    let source_ref = format!("{source_id}:{slot_name}");

    Ok((
        Some(SkillRecord {
            source_id: source_id.to_owned(),
            source_ref,
            id: frontmatter.id,
            slot_name,
            path: skill_dir.to_path_buf(),
            skill_file,
            description: frontmatter.description,
            requires: frontmatter.requires,
            owners: frontmatter.owners,
            tags: frontmatter.tags,
        }),
        warnings,
    ))
}

fn parse_frontmatter(markdown: &str, path: &Path) -> (SkillFrontmatter, Vec<InventoryWarning>) {
    let mut warnings = Vec::new();
    // Accept both LF and CRLF after the opening `---` fence so skills authored
    // on Windows parse the same as Unix ones.
    let opened = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"));
    let Some(rest) = opened else {
        return (SkillFrontmatter::default(), warnings);
    };

    let Some(end_index) = rest.find("\n---") else {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: "frontmatter start marker has no matching end marker".to_owned(),
        });
        return (SkillFrontmatter::default(), warnings);
    };

    let yaml = &rest[..end_index];
    match serde_yaml::from_str::<SkillFrontmatter>(yaml) {
        Ok(frontmatter) => (frontmatter, warnings),
        Err(error) => {
            warnings.push(InventoryWarning {
                code: InventoryWarningCode::MalformedFrontmatter,
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            (SkillFrontmatter::default(), warnings)
        }
    }
}

/// Resolve the slot name for a skill, or `None` when the skill must be skipped.
///
/// The front-matter `name` wins when valid. Otherwise the directory name is the
/// fallback, but it has to clear the same `is_valid_slot_name` bar because it
/// also becomes a path component under each target (a dir like `.config` would
/// otherwise create `~/.claude/skills/.config`). An invalid fallback yields an
/// `InvalidSlotName` warning and a `None`, so the caller drops the skill.
fn select_slot_name(
    frontmatter: &SkillFrontmatter,
    folder_name: &str,
    path: &Path,
    warnings: &mut Vec<InventoryWarning>,
) -> Option<String> {
    if let Some(name) = frontmatter.name.as_deref() {
        let trimmed = name.trim();
        if is_valid_slot_name(trimmed) {
            return Some(trimmed.to_owned());
        }

        warnings.push(InventoryWarning {
            code: InventoryWarningCode::InvalidSlotName,
            path: path.to_path_buf(),
            message: format!("frontmatter name `{name}` is not a valid slot name"),
        });
    }

    if is_valid_slot_name(folder_name) {
        return Some(folder_name.to_owned());
    }

    warnings.push(InventoryWarning {
        code: InventoryWarningCode::InvalidSlotName,
        path: path.to_path_buf(),
        message: format!("folder name `{folder_name}` is not a valid slot name"),
    });
    None
}

fn is_valid_slot_name(value: &str) -> bool {
    // A slot name becomes a single path component under each target directory,
    // so keep the accepted language conservative and cross-platform: lowercase
    // ASCII tokens only, no hidden/traversal segments, no trailing dots, and no
    // Windows device basenames.
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.starts_with('.')
        || value.ends_with('.')
        || is_windows_reserved_basename(value)
    {
        return false;
    }

    value.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'
            || character == '_'
            || character == '.'
    })
}

fn is_windows_reserved_basename(value: &str) -> bool {
    let basename = value.split('.').next().unwrap_or(value);
    matches!(
        basename,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn duplicate_slot_warnings(source_id: &str, skills: &[SkillRecord]) -> Vec<InventoryWarning> {
    let mut paths_by_slot: BTreeMap<&str, Vec<&Path>> = BTreeMap::new();
    for skill in skills {
        paths_by_slot
            .entry(&skill.slot_name)
            .or_default()
            .push(&skill.path);
    }

    paths_by_slot
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .flat_map(|(slot_name, paths)| {
            paths.into_iter().map(move |path| InventoryWarning {
                code: InventoryWarningCode::DuplicateSlotName,
                path: path.to_path_buf(),
                message: format!(
                    "source `{source_id}` contains multiple skills with slot name `{slot_name}`"
                ),
            })
        })
        .collect()
}

fn warning_code_name(code: InventoryWarningCode) -> &'static str {
    match code {
        InventoryWarningCode::MalformedFrontmatter => "malformed_frontmatter",
        InventoryWarningCode::InvalidSlotName => "invalid_slot_name",
        InventoryWarningCode::DuplicateSlotName => "duplicate_slot_name",
        InventoryWarningCode::UnreadablePath => "unreadable_path",
    }
}

impl std::fmt::Display for InventoryWarningCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(warning_code_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_source_should_find_skill_with_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("copy-editing");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nid: team.copy-editing\nname: copy-editing\ndescription: Edit copy\nrequires:\n  - style-guide\nowners:\n  - docs\ntags:\n  - writing\n---\n# Copy Editing\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let skill = &inventory.skills[0];
        assert_eq!(skill.source_ref, "company:copy-editing");
        assert_eq!(skill.id.as_deref(), Some("team.copy-editing"));
        assert_eq!(skill.requires, ["style-guide".to_owned()]);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_fallback_to_folder_name_when_frontmatter_name_is_invalid() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("release-notes.local");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: release notes\n---\n# Release Notes\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("local", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "release-notes.local");
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_report_duplicate_slot_names() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        for dir_name in ["first", "second"] {
            let skill_dir = temp_dir.path().join(dir_name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                "---\nname: shared\n---\n# Shared\n",
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");
        let duplicate_warnings = inventory
            .warnings
            .iter()
            .filter(|warning| warning.code == InventoryWarningCode::DuplicateSlotName)
            .count();

        assert_eq!(duplicate_warnings, 2);
    }

    #[test]
    fn scan_source_should_treat_supporting_files_as_part_of_one_skill() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Review\n").expect("skill file should be written");
        fs::write(skill_dir.join("guide.md"), "supporting").expect("guide should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        // Supporting files live next to `SKILL.md`; they must not spawn extra skill
        // records. Content fingerprints over those files return in V1.1 (drift
        // detection), persisted into the user lock.
        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].source_ref, "company:review");
    }

    #[test]
    fn is_valid_slot_name_should_reject_dot_segments() {
        assert!(!is_valid_slot_name("."));
        assert!(!is_valid_slot_name(".."));
        assert!(!is_valid_slot_name(".config"));
        assert!(!is_valid_slot_name("review."));
    }

    #[test]
    fn is_valid_slot_name_should_reject_non_portable_names() {
        let invalid_names = [
            "Review",
            "review copy",
            "review\ncopy",
            "caf\u{e9}",
            "cafe\u{301}",
            "con",
            "con.skill",
            "aux",
            "nul",
            "com1",
            "lpt9",
        ];

        for name in invalid_names {
            assert!(!is_valid_slot_name(name), "{name} should be invalid");
        }
    }

    #[test]
    fn is_valid_slot_name_should_accept_cross_platform_tokens() {
        for name in ["review", "release-notes.local", "copy_editing", "skill.123"] {
            assert!(is_valid_slot_name(name), "{name} should be valid");
        }
    }

    #[test]
    fn scan_source_should_skip_skill_when_folder_name_is_invalid() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        // No front-matter `name`, so the slot name falls back to the folder name;
        // the space makes it an invalid slot name, so the skill must be dropped.
        let skill_dir = temp_dir.path().join("bad name");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# No Frontmatter Name\n")
            .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_skip_uppercase_folder_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("Review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Review\n").expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
        assert!(inventory.warnings[0].message.contains("Review"));
    }

    #[test]
    fn scan_source_should_skip_unicode_folder_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("caf\u{e9}");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "# Cafe\n").expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn select_slot_name_should_return_none_when_folder_name_is_invalid() {
        let frontmatter = SkillFrontmatter::default();
        let mut warnings = Vec::new();

        let slot_name = select_slot_name(
            &frontmatter,
            "bad name",
            Path::new("/tmp/bad name/SKILL.md"),
            &mut warnings,
        );

        assert!(slot_name.is_none());
        assert_eq!(warnings[0].code, InventoryWarningCode::InvalidSlotName);
    }

    #[test]
    fn scan_source_should_fallback_when_frontmatter_name_has_case_collision_risk() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("review");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: Review\n---\n# Review\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "review");
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::InvalidSlotName
        );
    }

    #[test]
    fn scan_source_should_reject_traversal_frontmatter_name() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("legit");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(skill_dir.join(SKILL_FILE), "---\nname: ..\n---\n# Legit\n")
            .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].slot_name, "legit");
    }

    #[test]
    fn scan_source_should_parse_crlf_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("copy-editing");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\r\nname: copy-editing\r\nid: team.copy-editing\r\n---\r\n# Copy Editing\r\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].id.as_deref(), Some("team.copy-editing"));
    }
}
