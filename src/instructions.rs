//! Instruction packs rendered into managed blocks of agent instruction files.
//!
//! A managed block is delimited by paired markers
//! `<!-- dalo:start <pack-id> -->` and `<!-- dalo:end <pack-id> -->`. Only the
//! bytes between a pack's markers are ever rewritten; everything outside any
//! managed block is preserved.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::lockfile::LockedInstructionPack;
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
fn find_block(content: &str, pack_id: &str) -> Option<(usize, usize)> {
    let start = start_marker(pack_id);
    let end = end_marker(pack_id);
    let start_idx = content.find(&start)?;
    let end_rel = content[start_idx..].find(&end)?;
    Some((start_idx, start_idx + end_rel + end.len()))
}

/// Render `body` into `content` as `pack_id`'s managed block.
///
/// When the block exists, only the bytes between its markers change. When it does
/// not, the block is appended, separated from existing content by a blank line.
/// Rendering the same body twice is idempotent.
#[must_use]
pub fn render_block(content: &str, pack_id: &str, body: &str) -> String {
    let block = format!(
        "{}\n{}\n{}",
        start_marker(pack_id),
        body.trim_matches('\n'),
        end_marker(pack_id)
    );
    match find_block(content, pack_id) {
        Some((start_idx, end_idx)) => {
            format!("{}{}{}", &content[..start_idx], block, &content[end_idx..])
        }
        None => append_block(content, &block),
    }
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
#[must_use]
pub fn remove_block(content: &str, pack_id: &str) -> String {
    let Some((start_idx, end_idx)) = find_block(content, pack_id) else {
        return content.to_owned();
    };
    let before_raw = &content[..start_idx];
    let before = before_raw.strip_suffix('\n').unwrap_or(before_raw);
    let after_raw = &content[end_idx..];
    let after = after_raw.strip_prefix('\n').unwrap_or(after_raw);
    match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_owned(),
        (_, true) if before_raw.ends_with('\n') => format!("{before}\n"),
        (_, true) => before.to_owned(),
        _ => format!("{before}\n{after}"),
    }
}

/// Whether `content` contains `pack_id`'s managed block.
#[must_use]
pub fn has_block(content: &str, pack_id: &str) -> bool {
    find_block(content, pack_id).is_some()
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
    let existing = fs::read_to_string(target).unwrap_or_default();
    let rendered = render_block(&existing, &pack.id, &pack.body);
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
    let existing = fs::read_to_string(target).unwrap_or_default();
    let action = if has_block(&existing, pack_id) {
        let updated = remove_block(&existing, pack_id);
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

fn write_target(target: &Path, content: &str) -> DaloResult<()> {
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACK: &str = "house-style";

    #[test]
    fn render_block_should_append_when_absent_and_be_idempotent() {
        let original = "# Project\n\nNotes.\n";
        let once = render_block(original, PACK, "Use tabs.");
        assert!(has_block(&once, PACK));
        assert!(once.starts_with("# Project\n\nNotes.\n"));
        // A second render with the same body changes nothing.
        let twice = render_block(&once, PACK, "Use tabs.");
        assert_eq!(once, twice);
    }

    #[test]
    fn render_block_should_only_touch_bytes_inside_markers() {
        let original = "TOP CONTENT\n\n<!-- dalo:start house-style -->\nold\n<!-- dalo:end house-style -->\n\nBOTTOM CONTENT\n";
        let updated = render_block(original, PACK, "new body");
        // Everything outside the block is byte-identical.
        assert!(updated.starts_with("TOP CONTENT\n\n"));
        assert!(updated.ends_with("\n\nBOTTOM CONTENT\n"));
        assert!(updated.contains("new body"));
        assert!(!updated.contains("old"));
    }

    #[test]
    fn remove_block_should_preserve_surrounding_content() {
        let original = "# Header\n\nIntro.\n";
        let rendered = render_block(original, PACK, "Body.");
        let removed = remove_block(&rendered, PACK);
        assert!(!has_block(&removed, PACK));
        // The user-owned content survives the round trip.
        assert!(removed.contains("# Header"));
        assert!(removed.contains("Intro."));
        assert!(!removed.contains("dalo:start"));
    }

    #[test]
    fn remove_block_should_keep_content_on_both_sides() {
        let original = "ABOVE\n\n<!-- dalo:start house-style -->\nbody\n<!-- dalo:end house-style -->\n\nBELOW\n";
        let removed = remove_block(original, PACK);
        assert!(removed.contains("ABOVE"));
        assert!(removed.contains("BELOW"));
        assert!(!removed.contains("dalo:"));
    }

    #[test]
    fn remove_block_should_noop_when_absent() {
        let original = "# Header\n\nNo blocks here.\n";
        assert_eq!(remove_block(original, PACK), original);
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
}
