//! Instruction packs rendered into managed blocks of agent instruction files.
//!
//! A managed block is delimited by paired markers
//! `<!-- dalo:start <pack-id> -->` and `<!-- dalo:end <pack-id> -->`. Only the
//! bytes between a pack's markers are ever rewritten; everything outside any
//! managed block is preserved.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};
use crate::lockfile::LockedInstructionPack;
use crate::source::{SourceConfig, SourceKind};
use crate::store::{self, StorePaths};

/// A versioned instruction pack: standing agent-facing conventions as Markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPack {
    /// Pack ID.
    pub id: String,
    /// Declared version, when present in frontmatter.
    pub version: Option<String>,
    /// Rendered Markdown body.
    pub body: String,
}

/// Report from enabling or disabling an instruction pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionPackReport {
    /// Pack ID.
    pub pack_id: String,
    /// Instruction-file target affected.
    pub target: PathBuf,
    /// What happened: `enabled`, `disabled`, or `unchanged`.
    pub action: String,
}

fn start_marker(pack_id: &str) -> String {
    format!("<!-- dalo:start {pack_id} -->")
}

fn end_marker(pack_id: &str) -> String {
    format!("<!-- dalo:end {pack_id} -->")
}

/// Byte offsets `(start, end)` spanning a pack's managed block, markers included.
fn find_block(content: &str, pack_id: &str) -> DaloResult<Option<(usize, usize)>> {
    let start = start_marker(pack_id);
    let end = end_marker(pack_id);
    let starts = content.match_indices(&start).collect::<Vec<_>>();
    let ends = content.match_indices(&end).collect::<Vec<_>>();

    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start_idx, _)], [(end_idx, _)]) if start_idx < end_idx => {
            Ok(Some((*start_idx, end_idx + end.len())))
        }
        ([], _) => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "end marker exists without a matching start marker".to_owned(),
        }),
        (_, []) => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "start marker exists without a matching end marker".to_owned(),
        }),
        _ => Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "expected exactly one ordered start/end marker pair".to_owned(),
        }),
    }
}

/// Render `body` into `content` as `pack_id`'s managed block.
///
/// When the block exists, only the bytes between its markers change. When it does
/// not, the block is appended, separated from existing content by a blank line.
/// Rendering the same body twice is idempotent.
pub fn render_block(content: &str, pack_id: &str, body: &str) -> DaloResult<String> {
    let block = format!(
        "{}\n{}\n{}",
        start_marker(pack_id),
        body.trim_matches('\n'),
        end_marker(pack_id)
    );
    Ok(match find_block(content, pack_id)? {
        Some((start_idx, end_idx)) => {
            format!("{}{}{}", &content[..start_idx], block, &content[end_idx..])
        }
        None => append_block(content, &block),
    })
}

fn append_block(content: &str, block: &str) -> String {
    if content.is_empty() {
        return format!("{block}\n");
    }
    // Normalize the seam to exactly one blank line before the appended block.
    let separator = if content.ends_with("\n\n") {
        ""
    } else if content.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{content}{separator}{block}\n")
}

/// Remove `pack_id`'s managed block, preserving content outside it. A single
/// separating newline on each side of the block is also dropped so removal leaves
/// no blank gap where the block used to be.
pub fn remove_block(content: &str, pack_id: &str) -> DaloResult<String> {
    let Some((start_idx, end_idx)) = find_block(content, pack_id)? else {
        return Ok(content.to_owned());
    };
    let before_raw = &content[..start_idx];
    let before = before_raw.strip_suffix('\n').unwrap_or(before_raw);
    let after_raw = &content[end_idx..];
    let after = after_raw.strip_prefix('\n').unwrap_or(after_raw);
    Ok(match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_owned(),
        (_, true) if before_raw.ends_with('\n') => format!("{before}\n"),
        (_, true) => before.to_owned(),
        _ => format!("{before}\n{after}"),
    })
}

/// Whether `content` contains `pack_id`'s managed block.
#[must_use]
pub fn has_block(content: &str, pack_id: &str) -> bool {
    find_block(content, pack_id).is_ok_and(|block| block.is_some())
}

/// Validate a pack ID (same character rules as a source ID).
fn is_valid_pack_id(pack_id: &str) -> bool {
    !pack_id.is_empty()
        && pack_id != "."
        && pack_id != ".."
        && pack_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

/// Optional `version:` line in the pack's leading lines.
fn parse_version(body: &str) -> Option<String> {
    body.lines()
        .take(5)
        .find_map(|line| line.trim().strip_prefix("version:"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Read a user-authored pack from `local/instructions/<id>.md`.
pub fn read_local_pack(paths: &StorePaths, pack_id: &str) -> DaloResult<InstructionPack> {
    if !is_valid_pack_id(pack_id) {
        return Err(DaloError::InvalidSourceId {
            id: pack_id.to_owned(),
            reason: "instruction pack id must be `[A-Za-z0-9._-]` and not `.`/`..`".to_owned(),
        });
    }
    let path = paths.local_instructions_dir.join(format!("{pack_id}.md"));
    let body = fs::read_to_string(&path).map_err(|_| DaloError::SkillNotFound {
        skill: format!("instruction-pack:{pack_id}"),
    })?;
    Ok(InstructionPack {
        id: pack_id.to_owned(),
        version: parse_version(&body),
        body,
    })
}

/// Enable a local pack: render its managed block into `target` and record it in
/// the user lock. Idempotent: enabling an already-active pack re-renders the block
/// and updates the lock entry in place.
pub fn enable_pack(
    paths: &StorePaths,
    pack_id: &str,
    target: &Path,
) -> DaloResult<InstructionPackReport> {
    let pack = read_local_pack(paths, pack_id)?;
    let existing = read_target(target)?;
    let rendered = render_block(&existing, &pack.id, &pack.body)?;
    write_target(target, &rendered)?;

    let mut lock = store::read_user_lock(paths)?;
    lock.active_instruction_packs
        .retain(|entry| !(entry.pack_id == pack.id && entry.target == target));
    lock.active_instruction_packs.push(LockedInstructionPack {
        pack_id: pack.id.clone(),
        target: target.to_path_buf(),
        source_id: "local".to_owned(),
        commit: None,
        version: pack.version,
    });
    lock.active_instruction_packs.sort_by(|left, right| {
        left.pack_id
            .cmp(&right.pack_id)
            .then(left.target.cmp(&right.target))
    });
    store::write_user_lock(paths, &lock)?;

    Ok(InstructionPackReport {
        pack_id: pack.id,
        target: target.to_path_buf(),
        action: "enabled".to_owned(),
    })
}

/// Disable a pack: remove its managed block from `target` and drop its lock entry.
pub fn disable_pack(
    paths: &StorePaths,
    pack_id: &str,
    target: &Path,
) -> DaloResult<InstructionPackReport> {
    let existing = read_target(target)?;
    let action = if find_block(&existing, pack_id)?.is_some() {
        let updated = remove_block(&existing, pack_id)?;
        write_target(target, &updated)?;
        "disabled"
    } else {
        "unchanged"
    };

    let mut lock = store::read_user_lock(paths)?;
    let before = lock.active_instruction_packs.len();
    lock.active_instruction_packs
        .retain(|entry| !(entry.pack_id == pack_id && entry.target == target));
    if lock.active_instruction_packs.len() != before {
        store::write_user_lock(paths, &lock)?;
    }

    Ok(InstructionPackReport {
        pack_id: pack_id.to_owned(),
        target: target.to_path_buf(),
        action: action.to_owned(),
    })
}

fn read_target(target: &Path) -> DaloResult<String> {
    match fs::read_to_string(target) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_target(target: &Path, content: &str) -> DaloResult<()> {
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp_file = NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;
    temp_file.persist(target).map_err(|error| error.error)?;
    Ok(())
}

/// A discovered instruction pack (read-only inventory entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredPack {
    /// Pack ID.
    pub id: String,
    /// Source the pack was discovered in.
    pub source_id: String,
    /// Declared version, when present.
    pub version: Option<String>,
    /// Declared topics/tags.
    pub topics: Vec<String>,
    /// Whether the pack is currently enabled (active in the user lock).
    pub enabled: bool,
}

impl DiscoveredPack {
    /// Source-qualified pack ref.
    #[must_use]
    pub fn pack_ref(&self) -> String {
        format!("{}:{}", self.source_id, self.id)
    }
}

/// A topic overlap between two active instruction packs (advisory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicOverlap {
    /// The two overlapping pack refs.
    pub packs: [String; 2],
    /// The topics they share.
    pub topics: Vec<String>,
}

/// Discover instruction packs across the local store and configured sources.
///
/// Read-only: never materializes a pack. A pack is `enabled` when an active lock
/// entry matches its source and ID.
#[must_use]
pub fn discover_packs(
    paths: &StorePaths,
    sources: &[SourceConfig],
    active: &[LockedInstructionPack],
) -> Vec<DiscoveredPack> {
    let enabled: BTreeSet<(&str, &str)> = active
        .iter()
        .map(|entry| (entry.source_id.as_str(), entry.pack_id.as_str()))
        .collect();
    let mut packs = Vec::new();
    scan_pack_dir(&paths.local_instructions_dir, "local", &enabled, &mut packs);
    for source in sources {
        // The local source's instructions dir is the one scanned above; skip it so
        // local packs are not counted twice.
        if source.kind == SourceKind::Local {
            continue;
        }
        scan_pack_dir(
            &source.path.join("instructions"),
            &source.id,
            &enabled,
            &mut packs,
        );
    }
    packs.sort_by(|left, right| {
        left.source_id
            .cmp(&right.source_id)
            .then_with(|| left.id.cmp(&right.id))
    });
    packs
}

fn scan_pack_dir(
    dir: &Path,
    source_id: &str,
    enabled: &BTreeSet<(&str, &str)>,
    out: &mut Vec<DiscoveredPack>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !is_valid_pack_id(id) {
            continue;
        }
        let body = fs::read_to_string(&path).unwrap_or_default();
        out.push(DiscoveredPack {
            id: id.to_owned(),
            source_id: source_id.to_owned(),
            version: parse_version(&body),
            topics: parse_topics(&body),
            enabled: enabled.contains(&(source_id, id)),
        });
    }
}

/// Optional `topics:`/`tags:` line in the pack's leading lines (comma-separated).
fn parse_topics(body: &str) -> Vec<String> {
    body.lines()
        .take(8)
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("topics:")
                .or_else(|| trimmed.strip_prefix("tags:"))
        })
        .map(|value| {
            value
                .split(',')
                .map(|topic| topic.trim().to_owned())
                .filter(|topic| !topic.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Detect declared-topic overlaps among active packs. Advisory only: overlaps
/// never block materialization.
#[must_use]
pub fn topic_overlaps(active: &[DiscoveredPack]) -> Vec<TopicOverlap> {
    let mut overlaps = Vec::new();
    for (index, left) in active.iter().enumerate() {
        for right in active.iter().skip(index + 1) {
            let mut shared: Vec<String> = left
                .topics
                .iter()
                .filter(|topic| right.topics.contains(topic))
                .cloned()
                .collect();
            if !shared.is_empty() {
                shared.sort();
                shared.dedup();
                overlaps.push(TopicOverlap {
                    packs: [left.pack_ref(), right.pack_ref()],
                    topics: shared,
                });
            }
        }
    }
    overlaps
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::*;

    const PACK: &str = "house-style";

    #[test]
    fn render_block_should_append_when_absent_and_be_idempotent() {
        let original = "# Project\n\nNotes.\n";
        let once = render_block(original, PACK, "Use tabs.").expect("render should succeed");
        assert!(has_block(&once, PACK));
        assert!(once.starts_with("# Project\n\nNotes.\n"));
        // A second render with the same body changes nothing.
        let twice = render_block(&once, PACK, "Use tabs.").expect("render should succeed");
        assert_eq!(once, twice);
    }

    #[test]
    fn render_block_should_only_touch_bytes_inside_markers() {
        let original = "TOP CONTENT\n\n<!-- dalo:start house-style -->\nold\n<!-- dalo:end house-style -->\n\nBOTTOM CONTENT\n";
        let updated = render_block(original, PACK, "new body").expect("render should succeed");
        // Everything outside the block is byte-identical.
        assert!(updated.starts_with("TOP CONTENT\n\n"));
        assert!(updated.ends_with("\n\nBOTTOM CONTENT\n"));
        assert!(updated.contains("new body"));
        assert!(!updated.contains("old"));
    }

    #[test]
    fn remove_block_should_preserve_surrounding_content() {
        let original = "# Header\n\nIntro.\n";
        let rendered = render_block(original, PACK, "Body.").expect("render should succeed");
        let removed = remove_block(&rendered, PACK).expect("remove should succeed");
        assert!(!has_block(&removed, PACK));
        // The user-owned content survives the round trip.
        assert!(removed.contains("# Header"));
        assert!(removed.contains("Intro."));
        assert!(!removed.contains("dalo:start"));
    }

    #[test]
    fn remove_block_should_keep_content_on_both_sides() {
        let original = "ABOVE\n\n<!-- dalo:start house-style -->\nbody\n<!-- dalo:end house-style -->\n\nBELOW\n";
        let removed = remove_block(original, PACK).expect("remove should succeed");
        assert!(removed.contains("ABOVE"));
        assert!(removed.contains("BELOW"));
        assert!(!removed.contains("dalo:"));
    }

    #[test]
    fn remove_block_should_noop_when_absent() {
        let original = "# Header\n\nNo blocks here.\n";
        assert_eq!(
            remove_block(original, PACK).expect("remove should succeed"),
            original
        );
    }

    #[test]
    fn render_block_should_reject_malformed_markers() {
        let malformed = "# Header\n\n<!-- dalo:start house-style -->\nmissing end\n";
        let error = render_block(malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn parse_version_should_read_leading_version_line() {
        assert_eq!(
            parse_version("version: 1.2.0\n\n# Body\n"),
            Some("1.2.0".to_owned())
        );
        assert_eq!(parse_version("# Body only\n"), None);
    }

    #[test]
    fn is_valid_pack_id_should_reject_traversal() {
        assert!(is_valid_pack_id("house-style"));
        assert!(!is_valid_pack_id(".."));
        assert!(!is_valid_pack_id("bad/slash"));
        assert!(!is_valid_pack_id(""));
    }

    #[test]
    fn parse_topics_should_split_comma_separated_tags() {
        assert_eq!(
            parse_topics("topics: review, formatting, git\n\n# Body\n"),
            vec!["review", "formatting", "git"]
        );
        assert_eq!(parse_topics("tags: a,b\n"), vec!["a", "b"]);
        assert!(parse_topics("# No topics\n").is_empty());
    }

    fn discovered(id: &str, source: &str, topics: &[&str], enabled: bool) -> DiscoveredPack {
        DiscoveredPack {
            id: id.to_owned(),
            source_id: source.to_owned(),
            version: None,
            topics: topics.iter().map(|topic| (*topic).to_owned()).collect(),
            enabled,
        }
    }

    #[test]
    fn topic_overlaps_should_name_both_packs_sharing_a_topic() {
        let active = vec![
            discovered("style", "local", &["formatting", "tone"], true),
            discovered("format", "team", &["formatting"], true),
        ];
        let overlaps = topic_overlaps(&active);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(
            overlaps[0].packs,
            ["local:style".to_owned(), "team:format".to_owned()]
        );
        assert_eq!(overlaps[0].topics, vec!["formatting".to_owned()]);
    }

    #[test]
    fn topic_overlaps_should_ignore_disjoint_topics() {
        let active = vec![
            discovered("a", "local", &["security"], true),
            discovered("b", "team", &["formatting"], true),
        ];
        assert!(topic_overlaps(&active).is_empty());
    }

    #[test]
    fn discover_packs_should_find_local_packs_and_mark_enabled() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp.path().to_path_buf());
        fs::create_dir_all(&paths.local_instructions_dir).expect("dir should be created");
        fs::write(
            paths.local_instructions_dir.join("house.md"),
            "topics: x\n\nBody\n",
        )
        .expect("pack should be written");
        let active = vec![LockedInstructionPack {
            pack_id: "house".to_owned(),
            target: PathBuf::from("/tmp/AGENTS.md"),
            source_id: "local".to_owned(),
            commit: None,
            version: None,
        }];

        let packs = discover_packs(&paths, &[], &active);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "house");
        assert!(packs[0].enabled);
        assert_eq!(packs[0].topics, vec!["x".to_owned()]);
    }

    #[test]
    fn read_target_should_treat_missing_file_as_empty_only() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let missing = temp.path().join("AGENTS.md");

        assert_eq!(
            read_target(&missing).expect("missing target should read as empty"),
            ""
        );
    }

    #[test]
    fn write_target_should_replace_file_via_temp_rename() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let target = temp.path().join("AGENTS.md");
        fs::write(&target, "old\n").expect("target should be seeded");
        let before_inode = fs::metadata(&target)
            .expect("target metadata should be readable")
            .ino();

        write_target(&target, "new\n").expect("target should be written");

        assert_eq!(
            fs::read_to_string(&target).expect("target should be readable"),
            "new\n"
        );
        let after_inode = fs::metadata(&target)
            .expect("target metadata should be readable")
            .ino();
        assert_ne!(before_inode, after_inode);
        assert_eq!(
            fs::read_dir(temp.path())
                .expect("parent dir should be readable")
                .count(),
            1
        );
    }
}
