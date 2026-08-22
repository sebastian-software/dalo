//! Structurally owned provider hook sidecars with compare-and-swap updates.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};
use crate::hook::{HookProvider, NativeHookProjection};
use crate::store::StorePaths;

const SIDECAR_STATE_SCHEMA_VERSION: u32 = 1;

/// Planned or completed sidecar operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSidecarAction {
    /// A missing native file will be created.
    Create,
    /// Existing owned entries will be replaced structurally.
    Update,
    /// Native content already equals the desired projection.
    Noop,
    /// The last Dalo-owned entries will be removed.
    Remove,
}

impl std::fmt::Display for HookSidecarAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Noop => "noop",
            Self::Remove => "remove",
        })
    }
}

/// Exact read-only operation plan used unchanged by dry-run and apply.
#[derive(Debug, Clone, Serialize)]
pub struct HookSidecarPlan {
    /// Provider receiving the projection.
    pub provider: HookProvider,
    /// Native settings or hooks file.
    pub path: PathBuf,
    /// Planned operation.
    pub action: HookSidecarAction,
    /// Projection contract fingerprint.
    pub projection_fingerprint: String,
    /// Hash observed while planning, used for compare-and-swap.
    pub observed_hash: String,
    /// Number of portable hooks represented.
    pub portable_hooks: usize,
    /// Whether application is suppressed.
    pub dry_run: bool,
    #[serde(skip)]
    desired_bytes: Option<Vec<u8>>,
    #[serde(skip)]
    observed_bytes: Option<Vec<u8>>,
    #[serde(skip)]
    owned_hooks: BTreeMap<String, Vec<Value>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookSidecarStateFile {
    schema_version: u32,
    entries: Vec<HookSidecarState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookSidecarState {
    provider: HookProvider,
    path: PathBuf,
    projection_fingerprint: String,
    applied_file_hash: String,
    created_file: bool,
    owned_hooks: BTreeMap<String, Vec<Value>>,
}

/// Whether Dalo currently owns hook entries for this provider sidecar.
pub(crate) fn has_owned_entries(
    paths: &StorePaths,
    provider: HookProvider,
    path: &Path,
) -> DaloResult<bool> {
    Ok(read_state(paths)?.entries.iter().any(|entry| {
        entry.provider == provider && entry.path == path && !entry.owned_hooks.is_empty()
    }))
}

/// Build the exact reconcile plan without changing provider or store state.
pub fn plan_sidecar(
    paths: &StorePaths,
    provider: HookProvider,
    path: &Path,
    projection: &NativeHookProjection,
) -> DaloResult<HookSidecarPlan> {
    if projection.provider != provider {
        return Err(DaloError::InvalidArgument {
            reason: "hook projection provider does not match sidecar provider".to_owned(),
        });
    }
    let state = read_state(paths)?;
    let previous = state
        .entries
        .iter()
        .find(|entry| entry.provider == provider && entry.path == path);
    let observed_bytes = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let observed_hash = hash_optional(observed_bytes.as_deref());
    let mut root = match observed_bytes.as_deref() {
        Some(bytes) => {
            serde_json::from_slice::<Value>(bytes).map_err(|error| DaloError::StateError {
                reason: format!(
                    "malformed {provider:?} hook sidecar `{}`: {error}",
                    path.display()
                ),
            })?
        }
        None => Value::Object(serde_json::Map::new()),
    };
    if !root.is_object() {
        return Err(DaloError::StateError {
            reason: format!(
                "native {provider:?} hook sidecar `{}` must contain a JSON object",
                path.display()
            ),
        });
    }
    if let Some(previous) = previous {
        remove_owned_groups(&mut root, &previous.owned_hooks, path)?;
    } else if contains_untracked_dalo_dispatcher(&root) {
        return Err(DaloError::StateError {
            reason: format!(
                "native hook conflict in `{}`: Dalo dispatcher entries exist without ownership state",
                path.display()
            ),
        });
    }
    append_owned_groups(&mut root, &projection.hooks)?;
    let desired_bytes = if root.as_object().is_some_and(|object| object.is_empty()) {
        None
    } else {
        let mut bytes = serde_json::to_vec_pretty(&root)?;
        bytes.push(b'\n');
        Some(bytes)
    };
    let action = if desired_bytes == observed_bytes {
        HookSidecarAction::Noop
    } else if desired_bytes.is_none() {
        HookSidecarAction::Remove
    } else if observed_bytes.is_none() {
        HookSidecarAction::Create
    } else {
        HookSidecarAction::Update
    };
    Ok(HookSidecarPlan {
        provider,
        path: path.to_path_buf(),
        action,
        projection_fingerprint: projection.fingerprint.clone(),
        observed_hash,
        portable_hooks: projection.portable_hooks,
        dry_run: true,
        desired_bytes,
        observed_bytes,
        owned_hooks: projection.hooks.clone(),
    })
}

/// Apply the exact plan with native-file CAS and rollback if state persistence fails.
pub fn apply_sidecar(
    paths: &StorePaths,
    projection: &NativeHookProjection,
    mut plan: HookSidecarPlan,
    dry_run: bool,
) -> DaloResult<HookSidecarPlan> {
    plan.dry_run = dry_run;
    if dry_run {
        return Ok(plan);
    }
    fs::create_dir_all(&paths.hooks_dir)?;
    let manifest_path = paths
        .hooks_dir
        .join("projections")
        .join(format!("{}.json", projection.fingerprint));
    if !manifest_path.exists() {
        let mut manifest = serde_json::to_vec_pretty(&projection.dispatcher_manifest)?;
        manifest.push(b'\n');
        write_atomic(&manifest_path, &manifest)?;
    }
    let current = match fs::read(&plan.path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if hash_optional(current.as_deref()) != plan.observed_hash {
        return Err(DaloError::StateError {
            reason: format!(
                "concurrent edit detected for native hook sidecar `{}`; refusing to overwrite",
                plan.path.display()
            ),
        });
    }
    if plan.action != HookSidecarAction::Noop {
        write_or_remove(&plan.path, plan.desired_bytes.as_deref())?;
    }
    let applied_hash = hash_optional(plan.desired_bytes.as_deref());
    let mut state = match read_state(paths) {
        Ok(state) => state,
        Err(error) => {
            write_or_remove(&plan.path, plan.observed_bytes.as_deref())?;
            return Err(error);
        }
    };
    state
        .entries
        .retain(|entry| !(entry.provider == plan.provider && entry.path == plan.path));
    if !plan.owned_hooks.is_empty() {
        state.entries.push(HookSidecarState {
            provider: plan.provider,
            path: plan.path.clone(),
            projection_fingerprint: plan.projection_fingerprint.clone(),
            applied_file_hash: applied_hash,
            created_file: plan.observed_bytes.is_none(),
            owned_hooks: plan.owned_hooks.clone(),
        });
        state.entries.sort_by(|left, right| {
            format!("{:?}", left.provider)
                .cmp(&format!("{:?}", right.provider))
                .then(left.path.cmp(&right.path))
        });
    }
    if let Err(error) = write_state(paths, &state) {
        write_or_remove(&plan.path, plan.observed_bytes.as_deref())?;
        return Err(error);
    }
    Ok(plan)
}

fn read_state(paths: &StorePaths) -> DaloResult<HookSidecarStateFile> {
    match fs::read(&paths.hook_state_file) {
        Ok(bytes) => {
            let state: HookSidecarStateFile =
                serde_json::from_slice(&bytes).map_err(|error| DaloError::StateError {
                    reason: format!("malformed hook ownership state: {error}"),
                })?;
            if state.schema_version != SIDECAR_STATE_SCHEMA_VERSION {
                return Err(DaloError::StateError {
                    reason: format!(
                        "unsupported hook ownership schema {} (supported: {})",
                        state.schema_version, SIDECAR_STATE_SCHEMA_VERSION
                    ),
                });
            }
            Ok(state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HookSidecarStateFile {
            schema_version: SIDECAR_STATE_SCHEMA_VERSION,
            entries: Vec::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

fn write_state(paths: &StorePaths, state: &HookSidecarStateFile) -> DaloResult<()> {
    let mut bytes = serde_json::to_vec_pretty(state)?;
    bytes.push(b'\n');
    write_atomic(&paths.hook_state_file, &bytes)
}

fn remove_owned_groups(
    root: &mut Value,
    owned: &BTreeMap<String, Vec<Value>>,
    path: &Path,
) -> DaloResult<()> {
    let Some(root_object) = root.as_object_mut() else {
        unreachable!("caller validates root object");
    };
    let Some(hooks) = root_object.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Err(native_drift(path, "owned hooks section is missing"));
    };
    for (event, groups) in owned {
        let Some(native_groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            return Err(native_drift(
                path,
                &format!("owned event `{event}` is missing"),
            ));
        };
        for group in groups {
            let matches = native_groups
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| (candidate == group).then_some(index))
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(native_drift(
                    path,
                    &format!("owned event `{event}` was externally changed"),
                ));
            }
            native_groups.remove(matches[0]);
        }
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
    if hooks.is_empty() {
        root_object.remove("hooks");
    }
    Ok(())
}

fn append_owned_groups(root: &mut Value, owned: &BTreeMap<String, Vec<Value>>) -> DaloResult<()> {
    if owned.is_empty() {
        return Ok(());
    }
    let root_object = root.as_object_mut().expect("caller validates root object");
    let hooks = root_object
        .entry("hooks")
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| DaloError::StateError {
            reason: "native `hooks` key exists but is not a JSON object".to_owned(),
        })?;
    for (event, groups) in owned {
        let native_groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| DaloError::StateError {
                reason: format!("native hook event `{event}` exists but is not an array"),
            })?;
        native_groups.extend(groups.iter().cloned());
    }
    Ok(())
}

fn contains_untracked_dalo_dispatcher(root: &Value) -> bool {
    serde_json::to_string(root).is_ok_and(|content| {
        content.contains("--projection") && content.contains("dispatch") && content.contains("hook")
    })
}

fn native_drift(path: &Path, detail: &str) -> DaloError {
    DaloError::StateError {
        reason: format!(
            "native hook drift in `{}`: {detail}; refusing implicit overwrite",
            path.display()
        ),
    }
}

fn hash_optional(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        None => "missing".to_owned(),
    }
}

fn write_or_remove(path: &Path, bytes: Option<&[u8]>) -> DaloResult<()> {
    match bytes {
        Some(bytes) => write_atomic(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => {
                sync_parent(path)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> DaloResult<()> {
    let parent = path.parent().ok_or_else(|| DaloError::InvalidArgument {
        reason: format!("sidecar path `{}` has no parent", path.display()),
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn sync_parent(path: &Path) -> DaloResult<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (tempfile::TempDir, StorePaths) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        crate::store::init_store(root.clone(), false).unwrap();
        (temp, StorePaths::new(root))
    }

    fn projection(provider: HookProvider, fingerprint: &str, marker: &str) -> NativeHookProjection {
        let mut hooks = BTreeMap::new();
        if !marker.is_empty() {
            hooks.insert(
                "PreToolUse".to_owned(),
                vec![json!({
                    "matcher": "^(?:Bash)$",
                    "hooks": [{
                        "type": "command",
                        "command": "/dalo",
                        "args": ["hook", "dispatch", "--projection", marker]
                    }]
                })],
            );
        }
        NativeHookProjection {
            provider,
            provider_version: provider.baseline().to_owned(),
            fingerprint: fingerprint.to_owned(),
            portable_hooks: usize::from(!marker.is_empty()),
            hooks,
            dispatcher_manifest: json!({"schema_version": 1, "hooks": []}),
        }
    }

    #[test]
    fn create_noop_and_uninstall_are_exact_and_dry_run_is_inert() {
        let (temp, paths) = fixture();
        let sidecar = temp.path().join("codex/hooks.json");
        let desired = projection(HookProvider::Codex, &"11".repeat(32), "one");
        let dry_run = apply_sidecar(
            &paths,
            &desired,
            plan_sidecar(&paths, HookProvider::Codex, &sidecar, &desired).unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(dry_run.action, HookSidecarAction::Create);
        assert!(!sidecar.exists());

        let created = apply_sidecar(
            &paths,
            &desired,
            plan_sidecar(&paths, HookProvider::Codex, &sidecar, &desired).unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(created.action, HookSidecarAction::Create);
        let initial = fs::read(&sidecar).unwrap();
        let noop = plan_sidecar(&paths, HookProvider::Codex, &sidecar, &desired).unwrap();
        assert_eq!(noop.action, HookSidecarAction::Noop);

        let empty = projection(HookProvider::Codex, &"22".repeat(32), "");
        let removed = apply_sidecar(
            &paths,
            &empty,
            plan_sidecar(&paths, HookProvider::Codex, &sidecar, &empty).unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(removed.action, HookSidecarAction::Remove);
        assert!(!sidecar.exists());
        assert!(!initial.is_empty());
    }

    #[test]
    fn update_preserves_foreign_settings_and_new_concurrent_edits() {
        let (temp, paths) = fixture();
        let sidecar = temp.path().join("claude/settings.json");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(
            &sidecar,
            br#"{"permissions":{"allow":["Read"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"foreign"}]}]}}"#,
        )
        .unwrap();
        let first = projection(HookProvider::Claude, &"11".repeat(32), "one");
        apply_sidecar(
            &paths,
            &first,
            plan_sidecar(&paths, HookProvider::Claude, &sidecar, &first).unwrap(),
            false,
        )
        .unwrap();

        let mut externally_edited: Value =
            serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        externally_edited["theme"] = json!("dark");
        fs::write(
            &sidecar,
            serde_json::to_vec_pretty(&externally_edited).unwrap(),
        )
        .unwrap();
        let second = projection(HookProvider::Claude, &"22".repeat(32), "two");
        apply_sidecar(
            &paths,
            &second,
            plan_sidecar(&paths, HookProvider::Claude, &sidecar, &second).unwrap(),
            false,
        )
        .unwrap();
        let result: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        assert_eq!(result["permissions"]["allow"][0], "Read");
        assert_eq!(result["theme"], "dark");
        assert_eq!(result["hooks"]["Stop"][0]["hooks"][0]["command"], "foreign");
        assert_eq!(
            result["hooks"]["PreToolUse"][0]["hooks"][0]["args"][3],
            "two"
        );
    }

    #[test]
    fn malformed_sidecar_owned_drift_and_untracked_dispatcher_block() {
        let (temp, paths) = fixture();
        let sidecar = temp.path().join("settings.json");
        fs::write(&sidecar, "not json").unwrap();
        let desired = projection(HookProvider::Claude, &"11".repeat(32), "one");
        assert!(plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).is_err());

        fs::write(
            &sidecar,
            br#"{"hooks":{"PreToolUse":[{"hooks":[{"command":"dalo hook dispatch --projection orphan"}]}]}}"#,
        )
        .unwrap();
        assert!(plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).is_err());

        fs::remove_file(&sidecar).unwrap();
        apply_sidecar(
            &paths,
            &desired,
            plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).unwrap(),
            false,
        )
        .unwrap();
        let mut changed: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        changed["hooks"]["PreToolUse"][0]["matcher"] = json!("changed");
        fs::write(&sidecar, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).is_err());
    }

    #[test]
    fn compare_and_swap_rejects_a_post_plan_edit() {
        let (temp, paths) = fixture();
        let sidecar = temp.path().join("settings.json");
        fs::write(&sidecar, "{}").unwrap();
        let desired = projection(HookProvider::Claude, &"11".repeat(32), "one");
        let plan = plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).unwrap();
        fs::write(&sidecar, r#"{"external":true}"#).unwrap();
        assert!(apply_sidecar(&paths, &desired, plan, false).is_err());
        assert_eq!(
            fs::read_to_string(&sidecar).unwrap(),
            r#"{"external":true}"#
        );
    }

    #[test]
    fn state_write_failure_rolls_native_file_back() {
        let (temp, mut paths) = fixture();
        let sidecar = temp.path().join("settings.json");
        fs::write(&sidecar, r#"{"foreign":true}"#).unwrap();
        let desired = projection(HookProvider::Claude, &"11".repeat(32), "one");
        let plan = plan_sidecar(&paths, HookProvider::Claude, &sidecar, &desired).unwrap();
        let original = fs::read(&sidecar).unwrap();
        paths.hook_state_file = temp.path().join("state-as-directory");
        fs::create_dir(&paths.hook_state_file).unwrap();

        assert!(apply_sidecar(&paths, &desired, plan, false).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), original);
    }

    #[test]
    fn one_provider_conflict_does_not_corrupt_the_other_projection() {
        let (temp, paths) = fixture();
        let codex_path = temp.path().join("codex/hooks.json");
        let claude_path = temp.path().join("claude/settings.json");
        let codex = projection(HookProvider::Codex, &"11".repeat(32), "codex");
        apply_sidecar(
            &paths,
            &codex,
            plan_sidecar(&paths, HookProvider::Codex, &codex_path, &codex).unwrap(),
            false,
        )
        .unwrap();
        let codex_bytes = fs::read(&codex_path).unwrap();

        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        fs::write(&claude_path, "malformed").unwrap();
        let claude = projection(HookProvider::Claude, &"22".repeat(32), "claude");
        assert!(plan_sidecar(&paths, HookProvider::Claude, &claude_path, &claude).is_err());
        assert_eq!(fs::read(&codex_path).unwrap(), codex_bytes);
        assert_eq!(fs::read_to_string(&claude_path).unwrap(), "malformed");
    }
}
