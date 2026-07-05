//! Instruction packs rendered into managed blocks of agent instruction files.
//!
//! A managed block is delimited by paired markers
//! `<!-- dalo:start <pack-id> -->` and `<!-- dalo:end <pack-id> -->`. Only the
//! bytes between a pack's markers are ever rewritten; everything outside any
//! managed block is preserved.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Component;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};
use crate::lockfile::LockedInstructionPack;
use crate::source::{SourceConfig, SourceKind};
use crate::store::{self, StorePaths};

const START_MARKER_PREFIX: &str = "<!-- dalo:start ";
const END_MARKER_PREFIX: &str = "<!-- dalo:end ";

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
    /// Whether the command ran as dry-run.
    pub dry_run: bool,
}

/// Drift detected for an active instruction pack's rendered block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionBlockDrift {
    /// Source that owns the pack.
    pub source_id: String,
    /// Pack ID.
    pub pack_id: String,
    /// Instruction-file target that should contain the block.
    pub target: PathBuf,
    /// Drift kind.
    pub kind: InstructionBlockDriftKind,
    /// Human-readable detail.
    pub message: String,
}

/// Instruction block drift classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionBlockDriftKind {
    /// The expected managed block is absent.
    Missing,
    /// Markers are malformed, duplicated, or unreadable.
    Malformed,
    /// The block exists but no longer matches the current pack body.
    Stale,
    /// The active lock entry points to a pack that cannot be read.
    SourceMissing,
}

fn start_marker(pack_id: &str) -> String {
    format!("{START_MARKER_PREFIX}{pack_id} -->")
}

fn end_marker(pack_id: &str) -> String {
    format!("{END_MARKER_PREFIX}{pack_id} -->")
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
    let line_ending = line_ending_for(content);
    let block = render_managed_block_with_line_ending(pack_id, body, line_ending)?;
    Ok(match find_block(content, pack_id)? {
        Some((start_idx, end_idx)) => {
            format!("{}{}{}", &content[..start_idx], block, &content[end_idx..])
        }
        None => append_block(content, &block, line_ending),
    })
}

#[cfg(test)]
fn render_managed_block(pack_id: &str, body: &str) -> DaloResult<String> {
    render_managed_block_with_line_ending(pack_id, body, "\n")
}

fn render_managed_block_with_line_ending(
    pack_id: &str,
    body: &str,
    line_ending: &str,
) -> DaloResult<String> {
    validate_body_markers(pack_id, body)?;
    let body = normalize_line_endings(
        body.trim_matches(|character| character == '\n' || character == '\r'),
        line_ending,
    );
    Ok(format!(
        "{}{}{}{}{}",
        start_marker(pack_id),
        line_ending,
        body,
        line_ending,
        end_marker(pack_id)
    ))
}

fn validate_body_markers(pack_id: &str, body: &str) -> DaloResult<()> {
    if body.contains(START_MARKER_PREFIX) || body.contains(END_MARKER_PREFIX) {
        return Err(DaloError::MalformedInstructionBlock {
            pack_id: pack_id.to_owned(),
            reason: "instruction pack body contains dalo managed-block marker text".to_owned(),
        });
    }

    Ok(())
}

/// Check active instruction-pack lock entries against their rendered target blocks.
#[must_use]
pub fn instruction_block_drifts(
    paths: &StorePaths,
    sources: &[SourceConfig],
    active: &[LockedInstructionPack],
) -> Vec<InstructionBlockDrift> {
    let mut drifts = Vec::new();
    for entry in active {
        if let Some(drift) = instruction_block_drift(paths, sources, entry) {
            drifts.push(drift);
        }
    }
    drifts.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.source_id.cmp(&right.source_id))
            .then_with(|| left.pack_id.cmp(&right.pack_id))
    });
    drifts
}

fn instruction_block_drift(
    paths: &StorePaths,
    sources: &[SourceConfig],
    entry: &LockedInstructionPack,
) -> Option<InstructionBlockDrift> {
    let pack = match read_pack_for_lock_entry(paths, sources, entry) {
        Ok(pack) => pack,
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::SourceMissing,
                message: format!("active instruction pack source could not be read: {error}"),
            });
        }
    };
    let content = match fs::read_to_string(&entry.target) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::Missing,
                message: "instruction target file is missing".to_owned(),
            });
        }
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::Malformed,
                message: format!("instruction target file could not be read: {error}"),
            });
        }
    };
    let expected = match render_managed_block_with_line_ending(
        &entry.pack_id,
        &pack.body,
        line_ending_for(&content),
    ) {
        Ok(block) => block,
        Err(error) => {
            return Some(InstructionBlockDrift {
                source_id: entry.source_id.clone(),
                pack_id: entry.pack_id.clone(),
                target: entry.target.clone(),
                kind: InstructionBlockDriftKind::SourceMissing,
                message: format!("active instruction pack body is invalid: {error}"),
            });
        }
    };
    match find_block(&content, &entry.pack_id) {
        Ok(Some((start_idx, end_idx))) if content[start_idx..end_idx] == expected => None,
        Ok(Some(_)) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Stale,
            message: "instruction block does not match current pack body".to_owned(),
        }),
        Ok(None) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Missing,
            message: "instruction block is missing from target file".to_owned(),
        }),
        Err(error) => Some(InstructionBlockDrift {
            source_id: entry.source_id.clone(),
            pack_id: entry.pack_id.clone(),
            target: entry.target.clone(),
            kind: InstructionBlockDriftKind::Malformed,
            message: error.to_string(),
        }),
    }
}

fn read_pack_for_lock_entry(
    paths: &StorePaths,
    sources: &[SourceConfig],
    entry: &LockedInstructionPack,
) -> DaloResult<InstructionPack> {
    if entry.source_id == "local" {
        return read_local_pack(paths, &entry.pack_id);
    }

    let Some(source) = sources.iter().find(|source| source.id == entry.source_id) else {
        return Err(DaloError::UnknownSource {
            source_id: entry.source_id.clone(),
        });
    };
    read_pack_from_dir(&source.path.join("instructions"), &entry.pack_id)
}

fn append_block(content: &str, block: &str, line_ending: &str) -> String {
    if content.is_empty() {
        return format!("{block}{line_ending}");
    }
    // Normalize the seam to exactly one blank line before the appended block.
    let double_line_ending = format!("{line_ending}{line_ending}");
    let separator = if content.ends_with(&double_line_ending) {
        ""
    } else if content.ends_with(line_ending) {
        line_ending
    } else {
        return format!("{content}{line_ending}{line_ending}{block}{line_ending}");
    };
    format!("{content}{separator}{block}{line_ending}")
}

/// Remove `pack_id`'s managed block, preserving content outside it. A single
/// separating newline on each side of the block is also dropped so removal leaves
/// no blank gap where the block used to be.
pub fn remove_block(content: &str, pack_id: &str) -> DaloResult<String> {
    let Some((start_idx, end_idx)) = find_block(content, pack_id)? else {
        return Ok(content.to_owned());
    };
    let line_ending = line_ending_for(content);
    let before_raw = &content[..start_idx];
    let (before, before_had_line_ending) = strip_line_ending_suffix(before_raw, line_ending);
    let after_raw = &content[end_idx..];
    let (after, _) = strip_line_ending_prefix(after_raw, line_ending);
    Ok(match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_owned(),
        (_, true) if before_had_line_ending => format!("{before}{line_ending}"),
        (_, true) => before.to_owned(),
        _ if before.ends_with(line_ending) || after.starts_with(line_ending) => {
            format!("{before}{after}")
        }
        _ => format!("{before}{line_ending}{after}"),
    })
}

fn line_ending_for(content: &str) -> &'static str {
    let crlf_count = content.match_indices("\r\n").count();
    let bytes = content.as_bytes();
    let lf_count = bytes
        .iter()
        .enumerate()
        .filter(|(index, byte)| {
            **byte == b'\n' && (*index == 0 || bytes[index.saturating_sub(1)] != b'\r')
        })
        .count();
    if crlf_count > lf_count { "\r\n" } else { "\n" }
}

fn normalize_line_endings(content: &str, line_ending: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', line_ending)
}

fn strip_line_ending_suffix<'a>(content: &'a str, line_ending: &str) -> (&'a str, bool) {
    if let Some(stripped) = content.strip_suffix(line_ending) {
        return (stripped, true);
    }
    if line_ending != "\r\n"
        && let Some(stripped) = content.strip_suffix("\r\n")
    {
        return (stripped, true);
    }
    if let Some(stripped) = content.strip_suffix('\n') {
        return (stripped, true);
    }
    (content, false)
}

fn strip_line_ending_prefix<'a>(content: &'a str, line_ending: &str) -> (&'a str, bool) {
    if let Some(stripped) = content.strip_prefix(line_ending) {
        return (stripped, true);
    }
    if line_ending != "\r\n"
        && let Some(stripped) = content.strip_prefix("\r\n")
    {
        return (stripped, true);
    }
    if let Some(stripped) = content.strip_prefix('\n') {
        return (stripped, true);
    }
    (content, false)
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
    read_pack_from_dir(&paths.local_instructions_dir, pack_id)
}

fn read_pack_from_dir(dir: &Path, pack_id: &str) -> DaloResult<InstructionPack> {
    let path = dir.join(format!("{pack_id}.md"));
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
    dry_run: bool,
) -> DaloResult<InstructionPackReport> {
    let target = normalize_target_path(target)?;
    let pack = read_local_pack(paths, pack_id)?;
    let existing = read_target(&target)?;
    let rendered = render_block(&existing, &pack.id, &pack.body)?;
    if !dry_run {
        write_target(&target, &rendered)?;
    }

    let mut lock = store::read_user_lock(paths)?;
    if !dry_run {
        lock.active_instruction_packs
            .retain(|entry| !(entry.pack_id == pack.id && entry.target == target));
        lock.active_instruction_packs.push(LockedInstructionPack {
            pack_id: pack.id.clone(),
            target: target.clone(),
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
    }

    Ok(InstructionPackReport {
        pack_id: pack.id,
        target,
        action: "enabled".to_owned(),
        dry_run,
    })
}

/// Disable a pack: remove its managed block from `target` and drop its lock entry.
pub fn disable_pack(
    paths: &StorePaths,
    pack_id: &str,
    target: &Path,
    dry_run: bool,
) -> DaloResult<InstructionPackReport> {
    let target = normalize_target_path(target)?;
    let existing = read_target(&target)?;
    let has_block = find_block(&existing, pack_id)?.is_some();
    let mut lock = store::read_user_lock(paths)?;
    let before = lock.active_instruction_packs.len();
    let has_lock_entry = lock
        .active_instruction_packs
        .iter()
        .any(|entry| entry.pack_id == pack_id && entry.target == target);

    let action = if has_block {
        let updated = remove_block(&existing, pack_id)?;
        if !dry_run {
            write_target(&target, &updated)?;
        }
        "disabled"
    } else if has_lock_entry {
        "disabled"
    } else {
        "unchanged"
    };

    lock.active_instruction_packs
        .retain(|entry| !(entry.pack_id == pack_id && entry.target == target));
    if !dry_run && lock.active_instruction_packs.len() != before {
        store::write_user_lock(paths, &lock)?;
    }

    Ok(InstructionPackReport {
        pack_id: pack_id.to_owned(),
        target,
        action: action.to_owned(),
        dry_run,
    })
}

fn normalize_target_path(target: &Path) -> DaloResult<PathBuf> {
    let absolute = store::absolute_path(target)?;
    Ok(lexically_normalize(&absolute))
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    normalized
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
    use proptest::prelude::*;

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
    fn render_block_should_preserve_crlf_line_endings() {
        let original = "# Project\r\n\r\nNotes.\r\n";
        let rendered =
            render_block(original, PACK, "Use tabs.\nSecond line.").expect("render should succeed");

        assert!(rendered.contains("<!-- dalo:start house-style -->\r\n"));
        assert!(rendered.contains("Use tabs.\r\nSecond line."));
        assert!(!rendered.replace("\r\n", "").contains('\n'));
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

    proptest! {
        #[test]
        fn render_block_should_preserve_surrounding_text_and_stay_idempotent(
            prefix in "[A-Za-z0-9 ._\\n-]{0,80}",
            body in "[A-Za-z0-9 ._\\n-]{0,80}",
            suffix in "[A-Za-z0-9 ._\\n-]{0,80}",
        ) {
            let original = format!(
                "{}{}\\nold body\\n{}{}",
                prefix,
                start_marker(PACK),
                end_marker(PACK),
                suffix
            );

            let rendered = render_block(&original, PACK, &body).expect("render should succeed");
            prop_assert!(rendered.starts_with(&prefix));
            prop_assert!(rendered.ends_with(&suffix));
            prop_assert_eq!(
                render_block(&rendered, PACK, &body).expect("rerender should succeed"),
                rendered
            );
        }
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
    fn remove_block_should_preserve_crlf_seams() {
        let original = "ABOVE\r\n\r\n<!-- dalo:start house-style -->\r\nbody\r\n<!-- dalo:end house-style -->\r\n\r\nBELOW\r\n";
        let removed = remove_block(original, PACK).expect("remove should succeed");

        assert_eq!(removed, "ABOVE\r\n\r\nBELOW\r\n");
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
    fn render_block_should_reject_duplicate_start_markers() {
        let malformed = format!(
            "# Header\n\n{}\nold\n{}\nsecond\n{}\n",
            start_marker(PACK),
            start_marker(PACK),
            end_marker(PACK)
        );

        let error = render_block(&malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn render_block_should_reject_end_before_start() {
        let malformed = format!(
            "# Header\n\n{}\nold\n{}\n",
            end_marker(PACK),
            start_marker(PACK)
        );

        let error = render_block(&malformed, PACK, "Body.").expect_err("render should fail");

        assert!(matches!(error, DaloError::MalformedInstructionBlock { .. }));
    }

    #[test]
    fn render_block_should_replace_user_edits_inside_managed_block() {
        let rendered = render_block("# Header\n", PACK, "Original body.")
            .expect("initial render should succeed");
        let edited = rendered.replace("Original body.", "User edit inside block.");

        let updated =
            render_block(&edited, PACK, "Original body.").expect("rerender should succeed");

        assert_eq!(updated, rendered);
        assert!(!updated.contains("User edit inside block."));
    }

    #[test]
    fn render_block_should_handle_existing_block_at_eof_without_trailing_newline() {
        let content = format!("{}\nOld body\n{}", start_marker(PACK), end_marker(PACK));

        let updated = render_block(&content, PACK, "New body").expect("render should succeed");

        assert_eq!(updated, render_managed_block(PACK, "New body").unwrap());
    }

    #[test]
    fn render_block_should_reject_same_id_marker_in_body() {
        let body = format!("Do not emit this marker:\n{}\n", end_marker(PACK));

        let error = render_block("# Header\n", PACK, &body).expect_err("render should fail");

        assert_body_marker_error(error, PACK);
    }

    #[test]
    fn render_block_should_reject_different_id_marker_in_body() {
        let body = format!(
            "Example for another pack:\n{}\n",
            start_marker("other-pack")
        );

        let error = render_block("# Header\n", PACK, &body).expect_err("render should fail");

        assert_body_marker_error(error, PACK);
    }

    #[test]
    fn enable_pack_should_reject_marker_body_without_rewriting_target() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp.path().join("store");
        let target = temp.path().join("AGENTS.md");
        store::init_store(store_root.clone(), false).expect("store should be initialized");
        let paths = StorePaths::new(store_root);
        fs::write(
            paths.local_instructions_dir.join(format!("{PACK}.md")),
            format!("Before\n{}\nAfter\n", start_marker("other-pack")),
        )
        .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");

        let error = enable_pack(&paths, PACK, &target, false).expect_err("enable should fail");

        assert_body_marker_error(error, PACK);
        assert_eq!(
            fs::read_to_string(&target).expect("target should be readable"),
            "user-owned content\n"
        );
        let lock = store::read_user_lock(&paths).expect("lock should be readable");
        assert!(lock.active_instruction_packs.is_empty());
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

    fn assert_body_marker_error(error: DaloError, pack_id: &str) {
        let DaloError::MalformedInstructionBlock {
            pack_id: actual,
            reason,
        } = error
        else {
            panic!("expected malformed instruction block error");
        };
        assert_eq!(actual, pack_id);
        assert!(reason.contains("body contains dalo managed-block marker"));
    }
}
