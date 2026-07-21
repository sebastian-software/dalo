//! Skill inventory scanning.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentInventoryWarning, AgentRecord};
use crate::error::DaloResult;

const SKILL_FILE: &str = "SKILL.md";
const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;
const MAX_SKILL_METADATA_BYTES: usize = MAX_FRONTMATTER_BYTES + 16;
const MAX_FRONTMATTER_FLOW_DEPTH: usize = 64;

/// Inventory for one source checkout.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInventory {
    /// Source ID.
    pub source_id: String,
    /// Scanned skills.
    pub skills: Vec<SkillRecord>,
    /// Scanned canonical agent packages.
    pub agents: Vec<AgentRecord>,
    /// Non-fatal scan warnings.
    pub warnings: Vec<InventoryWarning>,
    /// Non-fatal canonical-agent package warnings.
    pub agent_warnings: Vec<AgentInventoryWarning>,
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
    /// A symlinked directory was skipped to avoid traversing outside the source
    /// or looping through a cycle.
    SkippedSymlink,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SkillFrontmatter {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    requires: Vec<String>,
    owners: Vec<String>,
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

    let agent_inventory = agent::scan_source_agents(source_id, source_root);

    Ok(SourceInventory {
        source_id: source_id.to_owned(),
        skills,
        agents: agent_inventory.agents,
        warnings,
        agent_warnings: agent_inventory.warnings,
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
            if file_type.is_symlink() {
                // Regular-file symlinks cannot contain a skill subtree, so they
                // are irrelevant to directory discovery. Repositories commonly
                // alias instruction files (for example CLAUDE.md -> AGENTS.md),
                // and treating those as a degraded skill inventory would make
                // otherwise compatible catalogs permanently unhealthy.
                if fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file()) {
                    continue;
                }
                warnings.push(InventoryWarning {
                    code: InventoryWarningCode::SkippedSymlink,
                    path: entry.path(),
                    message: "skipped symlink to keep the source scan bounded".to_owned(),
                });
            } else if file_type.is_dir() && !is_adoption_staging_dir_name(&entry.file_name()) {
                pending.push(entry.path());
            }
        }
    }

    found.sort();
    Ok(found)
}

fn is_adoption_staging_dir_name(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    let Some(rest) = name.strip_prefix('.') else {
        return false;
    };
    rest.rsplit_once(".dalo-adopting-")
        .is_some_and(|(skill_name, suffix)| !skill_name.is_empty() && !suffix.is_empty())
}

fn scan_skill(
    source_id: &str,
    skill_dir: &Path,
) -> DaloResult<(Option<SkillRecord>, Vec<InventoryWarning>)> {
    let skill_file = skill_dir.join(SKILL_FILE);
    let (skill_markdown, metadata_truncated) = read_skill_metadata(&skill_file)?;
    let (frontmatter, mut warnings) =
        parse_frontmatter(&skill_markdown, &skill_file, metadata_truncated);
    let Some(frontmatter) = frontmatter else {
        // Metadata participates in stable identity, approvals, and required
        // closure. Never silently activate a skill after losing those fields.
        return Ok((None, warnings));
    };
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

fn read_skill_metadata(path: &Path) -> io::Result<(String, bool)> {
    let file = fs::File::open(path)?;
    let metadata_truncated = file.metadata()?.len() > MAX_SKILL_METADATA_BYTES as u64;
    let mut bytes = Vec::with_capacity(MAX_SKILL_METADATA_BYTES);
    file.take(MAX_SKILL_METADATA_BYTES as u64)
        .read_to_end(&mut bytes)?;
    let markdown = match String::from_utf8(bytes) {
        Ok(markdown) => markdown,
        Err(error) if metadata_truncated && error.utf8_error().error_len().is_none() => {
            let valid_up_to = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid_up_to);
            String::from_utf8(bytes).expect("validated UTF-8 prefix should remain valid")
        }
        Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
    };
    Ok((markdown, metadata_truncated))
}

fn parse_frontmatter(
    markdown: &str,
    path: &Path,
    metadata_truncated: bool,
) -> (Option<SkillFrontmatter>, Vec<InventoryWarning>) {
    let mut warnings = Vec::new();
    // Accept both LF and CRLF after the opening `---` fence so skills authored
    // on Windows parse the same as Unix ones.
    let opened = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"));
    let Some(rest) = opened else {
        return (Some(SkillFrontmatter::default()), warnings);
    };

    let Some(end_index) = frontmatter_end_index(rest) else {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: if metadata_truncated {
                format!("frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte safety limit")
            } else {
                "frontmatter start marker has no matching end marker".to_owned()
            },
        });
        return (None, warnings);
    };

    let frontmatter = &rest[..end_index];
    if frontmatter.len() > MAX_FRONTMATTER_BYTES {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: format!("frontmatter exceeds the {MAX_FRONTMATTER_BYTES}-byte safety limit"),
        });
        return (None, warnings);
    }
    if frontmatter_flow_depth_exceeds(frontmatter, MAX_FRONTMATTER_FLOW_DEPTH) {
        warnings.push(InventoryWarning {
            code: InventoryWarningCode::MalformedFrontmatter,
            path: path.to_path_buf(),
            message: format!(
                "frontmatter flow nesting exceeds the {MAX_FRONTMATTER_FLOW_DEPTH}-level safety limit"
            ),
        });
        return (None, warnings);
    }
    match yaml_serde::from_str(frontmatter) {
        Ok(frontmatter) => (Some(frontmatter), warnings),
        Err(error) => {
            warnings.push(InventoryWarning {
                code: InventoryWarningCode::MalformedFrontmatter,
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            (None, warnings)
        }
    }
}

// Reject pathological flow collections before `yaml_serde` builds its event
// tree. This is intentionally a small lexical guard, not a second YAML parser;
// quoted scalars and comments cannot introduce structural nesting.
fn frontmatter_flow_depth_exceeds(frontmatter: &str, limit: usize) -> bool {
    let structural_frontmatter = frontmatter_without_block_scalar_bodies(frontmatter);
    let mut chars = structural_frontmatter.chars().peekable();
    let mut depth = 0_usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    let mut in_comment = false;
    let mut previous = None;

    while let Some(character) = chars.next() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            previous = Some(character);
            continue;
        }
        if in_single_quote {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_single_quote = false;
                }
            }
            previous = Some(character);
            continue;
        }
        if in_double_quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_double_quote = false;
            }
            previous = Some(character);
            continue;
        }

        match character {
            '#' if previous.is_none_or(char::is_whitespace) => in_comment = true,
            '\'' => in_single_quote = true,
            '"' => in_double_quote = true,
            '[' | '{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        previous = Some(character);
    }

    false
}

fn frontmatter_without_block_scalar_bodies(frontmatter: &str) -> String {
    let mut structural = String::with_capacity(frontmatter.len());
    let mut block_scalar_indent = None;

    for line in frontmatter.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let trimmed = content.trim_start_matches(' ');
        let indent = content.len() - trimmed.len();

        if let Some(header_indent) = block_scalar_indent {
            if trimmed.is_empty() || indent > header_indent {
                if line.ends_with('\n') {
                    structural.push('\n');
                }
                continue;
            }
            block_scalar_indent = None;
        }

        structural.push_str(line);
        if is_block_scalar_header(trimmed) {
            block_scalar_indent = Some(indent);
        }
    }

    structural
}

fn is_block_scalar_header(line: &str) -> bool {
    if line.starts_with('#') {
        return false;
    }
    let Some((_, value)) = line.split_once(':') else {
        return false;
    };
    let value = value.trim_start();
    let Some(indicator) = value.chars().next() else {
        return false;
    };
    if !matches!(indicator, '|' | '>') {
        return false;
    }
    value[indicator.len_utf8()..]
        .split('#')
        .next()
        .is_some_and(|suffix| {
            suffix
                .trim()
                .chars()
                .all(|character| matches!(character, '+' | '-' | '1'..='9'))
        })
}

fn frontmatter_end_index(rest: &str) -> Option<usize> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let line_without_cr = line_without_newline
            .strip_suffix('\r')
            .unwrap_or(line_without_newline);
        if line_without_cr == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
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
        InventoryWarningCode::SkippedSymlink => "skipped_symlink",
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
    use proptest::prelude::*;

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
    fn scan_source_should_parse_yaml_frontmatter_and_preserve_dependencies() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("app");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nid: example.app\nname: app\ndescription: >-\n  A valid folded YAML description.\nrequires: [missing-base]\nowners:\n  - \"team docs\"\nextra:\n  nested: accepted\n---\n# App\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills[0].id.as_deref(), Some("example.app"));
        assert_eq!(inventory.skills[0].requires, ["missing-base"]);
        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some("A valid folded YAML description.")
        );
    }

    #[test]
    fn scan_source_should_parse_yaml_description_scalar_styles() {
        let cases = [
            ("literal", "|", "First line.\nSecond line.\n"),
            ("folded", ">", "First line. Second line.\n"),
            ("stripped", ">-", "First line. Second line."),
        ];

        for (name, style, expected) in cases {
            let temp_dir = tempfile::tempdir().expect("tempdir should be created");
            let skill_dir = temp_dir.path().join(name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                format!(
                    "---\nname: {name}\ndescription: {style}\n  First line.\n  Second line.\n---\n# Skill\n"
                ),
            )
            .expect("skill file should be written");

            let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

            assert_eq!(inventory.skills[0].description.as_deref(), Some(expected));
            assert!(inventory.warnings.is_empty());
        }
    }

    #[test]
    fn scan_source_should_parse_plain_and_quoted_description_scalars() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        for (name, description) in [("plain", "Plain text"), ("quoted", "\"Quoted text\"")] {
            let skill_dir = temp_dir.path().join(name);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                format!("---\nname: {name}\ndescription: {description}\n---\n# Skill\n"),
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some("Plain text")
        );
        assert_eq!(
            inventory.skills[1].description.as_deref(),
            Some("Quoted text")
        );
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_warn_when_a_skill_directory_is_symlinked() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let shared_skill = temp_dir.path().join("shared-review");
        fs::create_dir_all(&source_root).expect("source root should be created");
        fs::create_dir_all(&shared_skill).expect("shared skill should be created");
        fs::write(
            shared_skill.join(SKILL_FILE),
            "---\nname: review\n---\n# Review\n",
        )
        .expect("skill file should be written");
        std::os::unix::fs::symlink(&shared_skill, source_root.join("review"))
            .expect("skill directory should be linked");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::SkippedSymlink
        );
    }

    #[test]
    fn scan_source_should_ignore_regular_file_symlinks() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        fs::create_dir_all(source_root.join("skills/review"))
            .expect("skill directory should be created");
        fs::write(source_root.join("AGENTS.md"), "# Instructions\n")
            .expect("instruction file should be written");
        std::os::unix::fs::symlink("AGENTS.md", source_root.join("CLAUDE.md"))
            .expect("instruction alias should be linked");
        fs::write(source_root.join("skills/review/SKILL.md"), "# Review\n")
            .expect("skill file should be written");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_warn_when_a_skill_symlink_is_broken() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        fs::create_dir_all(&source_root).expect("source root should be created");
        std::os::unix::fs::symlink(
            temp_dir.path().join("missing-review"),
            source_root.join("review"),
        )
        .expect("broken skill symlink should be created");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::SkippedSymlink
        );
        assert_eq!(inventory.warnings[0].path, source_root.join("review"));
    }

    #[test]
    fn scan_source_should_ignore_hidden_adoption_debris() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        for directory in ["review", ".review.dalo-adopting-interrupted"] {
            let skill_dir = source_root.join(directory);
            fs::create_dir_all(&skill_dir).expect("skill dir should be created");
            fs::write(
                skill_dir.join(SKILL_FILE),
                "---\nname: review\n---\n# Review\n",
            )
            .expect("skill file should be written");
        }

        let inventory = scan_source("local", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].path, source_root.join("review"));
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_allow_skills_in_hidden_directories() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source_root = temp_dir.path().join("checkout");
        let skill_dir = source_root.join("tools/.review");
        fs::create_dir_all(&skill_dir).expect("hidden skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: review\n---\n# Review\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", &source_root).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].path, skill_dir);
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_skip_malformed_frontmatter() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("app");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: [unterminated\n---\n# App\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
    }

    #[test]
    fn scan_source_should_reject_oversized_frontmatter_before_parsing() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("oversized");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: oversized\ndescription: {}\n---\n# Oversized\n",
                "x".repeat(MAX_FRONTMATTER_BYTES)
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
        assert!(inventory.warnings[0].message.contains("byte safety limit"));
    }

    #[test]
    fn scan_source_should_not_read_skill_body_beyond_the_metadata_window() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("bounded");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let mut markdown = b"---\nname: bounded\n---\n# Bounded\n".to_vec();
        markdown.extend("€".repeat(MAX_SKILL_METADATA_BYTES).as_bytes());
        markdown.push(0xff);
        fs::write(skill_dir.join(SKILL_FILE), markdown).expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(inventory.skills[0].slot_name, "bounded");
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_reject_deep_flow_nesting_before_parsing() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("nested");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let nesting = MAX_FRONTMATTER_FLOW_DEPTH + 1;
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: nested\ntags: {}value{}\n---\n# Nested\n",
                "[".repeat(nesting),
                "]".repeat(nesting)
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert!(inventory.skills.is_empty());
        assert_eq!(
            inventory.warnings[0].code,
            InventoryWarningCode::MalformedFrontmatter
        );
        assert!(
            inventory.warnings[0]
                .message
                .contains("flow nesting exceeds")
        );
    }

    #[test]
    fn frontmatter_flow_depth_guard_should_ignore_quotes_and_comments() {
        let delimiters = "[".repeat(MAX_FRONTMATTER_FLOW_DEPTH + 1);
        let frontmatter =
            format!("description: \"{delimiters}\"\nowner: '{delimiters}'\n# {delimiters}\n");

        assert!(!frontmatter_flow_depth_exceeds(
            &frontmatter,
            MAX_FRONTMATTER_FLOW_DEPTH
        ));
    }

    #[test]
    fn scan_source_should_allow_flow_delimiters_in_block_scalar_text() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("block-scalar");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        let delimiters = "[".repeat(MAX_FRONTMATTER_FLOW_DEPTH + 1);
        fs::write(
            skill_dir.join(SKILL_FILE),
            format!(
                "---\nname: block-scalar\ndescription: |-\n  {delimiters}\n---\n# Block Scalar\n"
            ),
        )
        .expect("skill file should be written");

        let inventory = scan_source("team", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        assert_eq!(
            inventory.skills[0].description.as_deref(),
            Some(delimiters.as_str())
        );
        assert!(inventory.warnings.is_empty());
    }

    #[test]
    fn scan_source_should_require_frontmatter_end_fence_line() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let skill_dir = temp_dir.path().join("workflow");
        fs::create_dir_all(&skill_dir).expect("skill dir should be created");
        fs::write(
            skill_dir.join(SKILL_FILE),
            "---\nname: workflow\nrequires:\n  - setup\nnotes: |\n  --- divider\n  ---- not a fence\nowners:\n  - team\n---\n# Workflow\n",
        )
        .expect("skill file should be written");

        let inventory = scan_source("company", temp_dir.path()).expect("scan should succeed");

        assert_eq!(inventory.skills.len(), 1);
        let skill = &inventory.skills[0];
        assert_eq!(skill.requires, ["setup".to_owned()]);
        assert_eq!(skill.owners, ["team".to_owned()]);
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

    proptest! {
        #[test]
        fn valid_slot_names_should_stay_portable(value in "\\PC{0,64}") {
            if is_valid_slot_name(&value) {
                prop_assert!(!value.is_empty());
                prop_assert_ne!(value.as_str(), ".");
                prop_assert_ne!(value.as_str(), "..");
                prop_assert!(!value.starts_with('.'));
                prop_assert!(!value.ends_with('.'));
                prop_assert!(!is_windows_reserved_basename(&value));
                let portable = value.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '-'
                        || character == '_'
                        || character == '.'
                });
                prop_assert!(portable);
            }
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
