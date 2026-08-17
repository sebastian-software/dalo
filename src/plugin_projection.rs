//! Provider-native package projection for selected portable plugins.
//!
//! Projection is intentionally separate from direct skill materialization:
//! ordinary skills keep their byte-identical symlink behavior, while every
//! selected portable plugin owns one independently removable native package
//! per supported provider.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{self as unix_fs, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static TEST_FAIL_STATE_WRITE: Cell<bool> = const { Cell::new(false) };
}

use crate::agent::{self, AgentProvider};
use crate::error::{DaloError, DaloResult};
use crate::hook::{HookStatusReport, HookTrustState};
use crate::inventory::SourceInventory;
use crate::plugin::{PluginComponentState, PluginResolution, PluginState, ResolvedPlugin};
use crate::store::{StateFile, StorePaths};
use crate::tool::{ToolState, ToolStatusReport};

const PROJECTION_SCHEMA_VERSION: u32 = 1;
const MAX_COMPONENT_ENTRIES: usize = 4096;
const MAX_COMPONENT_BYTES: u64 = 256 * 1024 * 1024;

/// One provider-native projection result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginTargetReport {
    /// Logical provider target.
    pub target: String,
    /// Canonical portable plugin identity.
    pub plugin: String,
    /// Stable provider-native namespace.
    pub native_name: String,
    /// Provider-visible installed package path.
    pub path: PathBuf,
    /// Immutable content-addressed artifact path.
    pub artifact_path: PathBuf,
    /// Native package manifest format and adapter baseline.
    pub adapter_baseline: String,
    /// Complete deterministic projection fingerprint.
    pub projection_hash: String,
    /// Planned/current result.
    pub state: PluginProjectionState,
    /// Component-level projection facts, including explicit omissions.
    pub components: Vec<PluginProjectionComponent>,
    /// Whether mutation was suppressed.
    pub dry_run: bool,
    /// Actionable detail.
    pub diagnostic: String,
}

/// Stable native package lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginProjectionState {
    /// Existing owned link and immutable bytes match.
    Ready,
    /// A dry-run would create or update the projection.
    Planned,
    /// Canonical plugin coherence blocks native projection.
    Blocked,
    /// Existing provider content is not owned by Dalo or has drifted.
    Conflict,
}

/// One canonical component's native package outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginProjectionComponent {
    /// Source-qualified component identity or authored reference.
    pub identity: String,
    /// Component kind.
    pub kind: String,
    /// Native relative path, when packaged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_path: Option<String>,
    /// Exact source/generated content fingerprint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Exact packaging state.
    pub state: String,
    /// Provenance or omission explanation.
    pub diagnostic: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProjectionStateFile {
    schema_version: u32,
    entries: Vec<ProjectionOwnership>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectionOwnership {
    target: String,
    plugin: String,
    native_name: String,
    link_path: PathBuf,
    artifact_path: PathBuf,
    projection_hash: String,
    adapter_baseline: String,
    components: Vec<PluginProjectionComponent>,
}

#[derive(Debug)]
struct RenderedPackage {
    temporary: TempDir,
    hash: String,
    components: Vec<PluginProjectionComponent>,
    baseline: &'static str,
}

#[derive(Debug)]
struct PendingProjection {
    rendered: RenderedPackage,
    ownership: ProjectionOwnership,
    current: bool,
}

#[derive(Debug)]
struct LinkSnapshot {
    path: PathBuf,
    target: Option<PathBuf>,
}

#[derive(Debug)]
struct LinkMutation {
    before: LinkSnapshot,
    expected_after: Option<PathBuf>,
}

/// Reconcile one independently owned native package for every selected plugin
/// and linked Codex/Claude target. No approval is granted by this operation.
pub fn reconcile(
    paths: &StorePaths,
    state: &StateFile,
    plugins: &PluginResolution,
    inventories: &[SourceInventory],
    tools: &[ToolStatusReport],
    hooks: &[HookStatusReport],
    dry_run: bool,
) -> DaloResult<Vec<PluginTargetReport>> {
    let previous = read_state(paths)?;
    let mut desired = Vec::new();
    let mut reports = Vec::new();
    let mut pending = Vec::new();
    for target in state
        .targets
        .iter()
        .filter(|target| target.enabled && matches!(target.id.as_str(), "codex" | "claude"))
    {
        for plugin in plugins
            .plugins
            .iter()
            .filter(|plugin| matches!(plugin.state, PluginState::Selected | PluginState::Blocked))
        {
            let native_name = native_name(&plugin.source_ref);
            let link_path = install_path(&target.id, &target.path, &native_name)?;
            if plugin.state == PluginState::Blocked {
                reports.push(PluginTargetReport {
                    target: target.id.clone(),
                    plugin: plugin.source_ref.clone(),
                    native_name,
                    path: link_path,
                    artifact_path: PathBuf::new(),
                    adapter_baseline: baseline(&target.id).to_owned(),
                    projection_hash: String::new(),
                    state: PluginProjectionState::Blocked,
                    components: Vec::new(),
                    dry_run,
                    diagnostic: plugin.blocking_reasons.join("; "),
                });
                continue;
            }
            let rendered = match render_package(
                paths,
                &target.id,
                plugin,
                inventories,
                tools,
                hooks,
                dry_run,
            ) {
                Ok(rendered) => rendered,
                Err(error) => {
                    reports.push(PluginTargetReport {
                        target: target.id.clone(),
                        plugin: plugin.source_ref.clone(),
                        native_name,
                        path: link_path,
                        artifact_path: PathBuf::new(),
                        adapter_baseline: baseline(&target.id).to_owned(),
                        projection_hash: String::new(),
                        state: PluginProjectionState::Blocked,
                        components: Vec::new(),
                        dry_run,
                        diagnostic: error.to_string(),
                    });
                    continue;
                }
            };
            if let Some(blocker) = active_component_blocker(plugin, tools, hooks) {
                reports.push(PluginTargetReport {
                    target: target.id.clone(),
                    plugin: plugin.source_ref.clone(),
                    native_name,
                    path: link_path,
                    artifact_path: PathBuf::new(),
                    adapter_baseline: rendered.baseline.to_owned(),
                    projection_hash: rendered.hash,
                    state: PluginProjectionState::Blocked,
                    components: rendered.components,
                    dry_run,
                    diagnostic: blocker,
                });
                continue;
            }
            let artifact_path = paths
                .plugins_dir
                .join("artifacts")
                .join(&target.id)
                .join(&rendered.hash);
            if fs::symlink_metadata(&artifact_path).is_ok()
                && (!artifact_path.is_dir() || hash_tree(&artifact_path)? != rendered.hash)
            {
                reports.push(PluginTargetReport {
                    target: target.id.clone(),
                    plugin: plugin.source_ref.clone(),
                    native_name,
                    path: link_path,
                    artifact_path,
                    adapter_baseline: rendered.baseline.to_owned(),
                    projection_hash: rendered.hash,
                    state: PluginProjectionState::Conflict,
                    components: rendered.components,
                    dry_run,
                    diagnostic: "content-addressed native artifact failed its hash audit"
                        .to_owned(),
                });
                if let Some(prior) = previous
                    .entries
                    .iter()
                    .find(|entry| entry.target == target.id && entry.plugin == plugin.source_ref)
                {
                    desired.push(prior.clone());
                }
                continue;
            }
            let ownership = ProjectionOwnership {
                target: target.id.clone(),
                plugin: plugin.source_ref.clone(),
                native_name: native_name.clone(),
                link_path: link_path.clone(),
                artifact_path: artifact_path.clone(),
                projection_hash: rendered.hash.clone(),
                adapter_baseline: rendered.baseline.to_owned(),
                components: rendered.components.clone(),
            };
            let prior = previous
                .entries
                .iter()
                .find(|entry| entry.target == target.id && entry.plugin == plugin.source_ref);
            let current_matches = owned_link_matches(prior, &link_path, &artifact_path);
            let owned_prior_matches = prior.is_some_and(|entry| {
                owned_link_matches(Some(entry), &link_path, &entry.artifact_path)
            });
            if (link_path.exists() || fs::symlink_metadata(&link_path).is_ok())
                && !owned_prior_matches
            {
                reports.push(report_for(
                    &ownership,
                    PluginProjectionState::Conflict,
                    dry_run,
                    "provider path contains foreign or drifted content",
                ));
                if let Some(prior) = prior {
                    desired.push(prior.clone());
                }
                continue;
            }
            desired.push(ownership.clone());
            reports.push(report_for(
                &ownership,
                if dry_run && !current_matches {
                    PluginProjectionState::Planned
                } else {
                    PluginProjectionState::Ready
                },
                dry_run,
                if current_matches {
                    "native package is current"
                } else {
                    "native package projected"
                },
            ));
            pending.push(PendingProjection {
                rendered,
                ownership,
                current: current_matches,
            });
        }
    }

    let stale = previous
        .entries
        .iter()
        .filter(|entry| {
            !desired.iter().any(|candidate| {
                candidate.target == entry.target && candidate.plugin == entry.plugin
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    for entry in &stale {
        if fs::symlink_metadata(&entry.link_path).is_ok()
            && !owned_link_matches(Some(entry), &entry.link_path, &entry.artifact_path)
        {
            return Err(state_error(format!(
                "owned plugin path `{}` drifted; refusing removal",
                entry.link_path.display()
            )));
        }
    }
    desired.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.plugin.cmp(&right.plugin))
    });
    if !dry_run {
        for projection in &pending {
            promote_artifact(
                &projection.rendered.temporary,
                &projection.ownership.artifact_path,
            )?;
        }
        let mut mutations = Vec::new();
        let apply = (|| -> DaloResult<()> {
            for projection in &pending {
                if projection.current {
                    continue;
                }
                let prior = previous.entries.iter().find(|entry| {
                    entry.target == projection.ownership.target
                        && entry.plugin == projection.ownership.plugin
                });
                let before = snapshot_link(&projection.ownership.link_path);
                replace_owned_link(
                    &projection.ownership.link_path,
                    &projection.ownership.artifact_path,
                    prior,
                )?;
                mutations.push(LinkMutation {
                    before,
                    expected_after: Some(projection.ownership.artifact_path.clone()),
                });
            }
            for entry in &stale {
                let existed = fs::symlink_metadata(&entry.link_path).is_ok();
                let before = snapshot_link(&entry.link_path);
                remove_owned_link(entry, false)?;
                if existed {
                    mutations.push(LinkMutation {
                        before,
                        expected_after: None,
                    });
                }
            }
            write_state(
                paths,
                &ProjectionStateFile {
                    schema_version: PROJECTION_SCHEMA_VERSION,
                    entries: desired,
                },
            )
        })();
        if let Err(error) = apply {
            if let Err(rollback) = restore_links(&mutations) {
                return Err(state_error(format!(
                    "{error}; native plugin rollback also failed: {rollback}"
                )));
            }
            return Err(error);
        }
    }
    reports.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.plugin.cmp(&right.plugin))
    });
    Ok(reports)
}

fn render_package(
    paths: &StorePaths,
    provider: &str,
    plugin: &ResolvedPlugin,
    inventories: &[SourceInventory],
    tools: &[ToolStatusReport],
    hooks: &[HookStatusReport],
    dry_run: bool,
) -> DaloResult<RenderedPackage> {
    let temporary = if dry_run {
        tempfile::tempdir()?
    } else {
        fs::create_dir_all(&paths.plugins_dir)?;
        tempfile::tempdir_in(&paths.plugins_dir)?
    };
    let root = temporary.path();
    let mut components = Vec::new();
    for member in &plugin.members {
        let identity = member.resolved_ref.as_deref().unwrap_or(&member.reference);
        let kind = member.reference.split(':').next().unwrap_or("unknown");
        if member.state != PluginComponentState::Active {
            components.push(PluginProjectionComponent {
                identity: identity.to_owned(),
                kind: kind.to_owned(),
                native_path: None,
                content_hash: None,
                state: "omitted".to_owned(),
                diagnostic: format!("canonical component is {:?}", member.state).to_lowercase(),
            });
            continue;
        }
        match kind {
            "skill" => {
                let skill = inventories.iter().flat_map(|inventory| &inventory.skills)
                    .find(|skill| skill.source_ref == identity)
                    .ok_or_else(|| state_error(format!("active skill `{identity}` is absent from inventory")))?;
                let relative = format!("skills/{}", skill.slot_name);
                copy_package_tree(&skill.path, &root.join(&relative))?;
                components.push(packaged_component(identity, kind, relative, &skill.path)?);
            }
            "agent" if provider == "claude" => {
                let record = inventories.iter().flat_map(|inventory| &inventory.agents)
                    .find(|agent| agent.source_ref == identity)
                    .ok_or_else(|| state_error(format!("active agent `{identity}` is absent from inventory")))?;
                let compilation = agent::compile_record(record, AgentProvider::Claude);
                let bytes = compilation.bytes.ok_or_else(|| state_error(format!(
                    "agent `{identity}` cannot be represented safely for Claude: {:?}", compilation.overall
                )))?;
                let relative = format!("agents/{}.md", record.slot_name);
                write_file(&root.join(&relative), bytes.as_bytes())?;
                components.push(PluginProjectionComponent {
                    identity: identity.to_owned(), kind: kind.to_owned(),
                    native_path: Some(relative), content_hash: Some(hash_bytes(bytes.as_bytes())),
                    state: "packaged".to_owned(), diagnostic: "compiled by the reviewed Claude agent adapter".to_owned(),
                });
            }
            "agent" => components.push(PluginProjectionComponent {
                identity: identity.to_owned(), kind: kind.to_owned(), native_path: None,
                content_hash: None, state: "external".to_owned(),
                diagnostic: "Codex has no general native plugin agent primitive; canonical agent remains an explicit external projection".to_owned(),
            }),
            "instruction" => components.push(PluginProjectionComponent {
                identity: identity.to_owned(), kind: kind.to_owned(), native_path: None,
                content_hash: None, state: "external".to_owned(),
                diagnostic: "instruction packs remain managed top-level guidance and are never injected into plugin prompts".to_owned(),
            }),
            _ => components.push(PluginProjectionComponent {
                identity: identity.to_owned(), kind: kind.to_owned(), native_path: None,
                content_hash: None, state: "unsupported".to_owned(), diagnostic: "unsupported component kind".to_owned(),
            }),
        }
    }
    append_active_components(plugin, tools, hooks, &mut components);
    let manifest_dir = root.join(if provider == "codex" {
        ".codex-plugin"
    } else {
        ".claude-plugin"
    });
    fs::create_dir_all(&manifest_dir)?;
    let authored = authored_plugin(inventories, &plugin.source_ref);
    let version = format!("0.0.0+dalo.{}", &plugin.package_hash[..12]);
    let description = authored.map_or_else(
        || "Dalo portable plugin projection".to_owned(),
        |record| record.description.clone(),
    );
    let mut manifest = serde_json::Map::from_iter([
        ("name".to_owned(), json!(native_name(&plugin.source_ref))),
        ("version".to_owned(), json!(version)),
        ("description".to_owned(), json!(description)),
    ]);
    if provider == "codex" && root.join("skills").is_dir() {
        manifest.insert("skills".to_owned(), json!("./skills/"));
    }
    let manifest = serde_json::Value::Object(manifest);
    write_json(&manifest_dir.join("plugin.json"), &manifest)?;
    components.sort_by(|left, right| {
        left.identity
            .cmp(&right.identity)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    let provenance = json!({
        "schema_version": PROJECTION_SCHEMA_VERSION,
        "portable_plugin": plugin.source_ref,
        "package_hash": plugin.package_hash,
        "closure_hash": plugin.closure_hash,
        "provider": provider,
        "adapter_baseline": baseline(provider),
        "authored_version": authored.and_then(|record| record.version.as_deref()),
        "components": components,
    });
    write_json(&root.join("dalo-provenance.json"), &provenance)?;
    let hash = hash_tree(root)?;
    Ok(RenderedPackage {
        temporary,
        hash,
        components,
        baseline: baseline(provider),
    })
}

fn append_active_components(
    plugin: &ResolvedPlugin,
    tools: &[ToolStatusReport],
    hooks: &[HookStatusReport],
    components: &mut Vec<PluginProjectionComponent>,
) {
    let prefix = format!("{}#tool:", plugin.source_ref);
    for tool in tools
        .iter()
        .filter(|tool| tool.tool.source_ref.starts_with(&prefix))
    {
        components.push(PluginProjectionComponent {
            identity: tool.tool.source_ref.clone(),
            kind: "tool".to_owned(),
            native_path: None,
            content_hash: Some(tool.tool.contract_hash.clone()),
            state: if tool.state == ToolState::Ready {
                "external_ready"
            } else {
                "blocked"
            }
            .to_owned(),
            diagnostic: format!(
                "separate exact tool trust is {:?}; immutable execution remains Dalo-dispatched",
                tool.state
            )
            .to_lowercase(),
        });
    }
    let prefix = format!("{}#hook:", plugin.source_ref);
    for hook in hooks
        .iter()
        .filter(|hook| hook.hook.source_ref.starts_with(&prefix))
    {
        components.push(PluginProjectionComponent {
            identity: hook.hook.source_ref.clone(),
            kind: "hook".to_owned(),
            native_path: None,
            content_hash: Some(hook.hook.contract_hash.clone()),
            state: if hook.state == HookTrustState::Ready {
                "external_ready"
            } else {
                "blocked"
            }
            .to_owned(),
            diagnostic: format!(
                "separate exact hook trust is {:?}; native sidecar ownership remains independent",
                hook.state
            )
            .to_lowercase(),
        });
    }
}

fn active_component_blocker(
    plugin: &ResolvedPlugin,
    tools: &[ToolStatusReport],
    hooks: &[HookStatusReport],
) -> Option<String> {
    let tool_prefix = format!("{}#tool:", plugin.source_ref);
    if let Some(tool) = tools.iter().find(|tool| {
        tool.tool.source_ref.starts_with(&tool_prefix)
            && tool.tool.availability == crate::plugin::ToolAvailability::Required
            && tool.state != ToolState::Ready
    }) {
        return Some(
            format!(
                "required tool `{}` is {:?}; packaging never grants execution authority",
                tool.tool.source_ref, tool.state
            )
            .to_lowercase(),
        );
    }
    let hook_prefix = format!("{}#hook:", plugin.source_ref);
    hooks
        .iter()
        .find(|hook| {
            hook.hook.source_ref.starts_with(&hook_prefix)
                && hook.hook.descriptor.requirement == crate::hook::HookRequirement::Required
                && hook.state != HookTrustState::Ready
        })
        .map(|hook| {
            format!(
                "required hook `{}` is {:?}; packaging preserves its separate approval",
                hook.hook.source_ref, hook.state
            )
            .to_lowercase()
        })
}

fn authored_plugin<'a>(
    inventories: &'a [SourceInventory],
    identity: &str,
) -> Option<&'a crate::plugin::PluginRecord> {
    inventories
        .iter()
        .flat_map(|inventory| &inventory.plugins)
        .find(|plugin| plugin.source_ref == identity)
}

fn packaged_component(
    identity: &str,
    kind: &str,
    relative: String,
    source: &Path,
) -> DaloResult<PluginProjectionComponent> {
    Ok(PluginProjectionComponent {
        identity: identity.to_owned(),
        kind: kind.to_owned(),
        native_path: Some(relative),
        content_hash: Some(hash_tree(source)?),
        state: "packaged".to_owned(),
        diagnostic: "canonical direct artifact copied without generator execution".to_owned(),
    })
}

fn baseline(provider: &str) -> &'static str {
    match provider {
        "codex" => "codex-plugin-v1",
        "claude" => "claude-plugin-v1",
        _ => "unsupported",
    }
}

fn native_name(identity: &str) -> String {
    let mut stem = String::new();
    for character in identity.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            stem.push(character);
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.len() > 55 { &stem[..55] } else { stem };
    format!("dalo-{stem}-{}", &hash_bytes(identity.as_bytes())[..12])
}

fn install_path(provider: &str, skills_path: &Path, name: &str) -> DaloResult<PathBuf> {
    if provider == "claude" {
        return Ok(skills_path.join(name));
    }
    let parent = skills_path
        .parent()
        .ok_or_else(|| state_error("Codex skills target has no parent directory".to_owned()))?;
    Ok(parent.join("plugins/dalo").join(name))
}

fn report_for(
    ownership: &ProjectionOwnership,
    state: PluginProjectionState,
    dry_run: bool,
    diagnostic: &str,
) -> PluginTargetReport {
    PluginTargetReport {
        target: ownership.target.clone(),
        plugin: ownership.plugin.clone(),
        native_name: ownership.native_name.clone(),
        path: ownership.link_path.clone(),
        artifact_path: ownership.artifact_path.clone(),
        adapter_baseline: ownership.adapter_baseline.clone(),
        projection_hash: ownership.projection_hash.clone(),
        state,
        components: ownership.components.clone(),
        dry_run,
        diagnostic: diagnostic.to_owned(),
    }
}

fn promote_artifact(temporary: &TempDir, destination: &Path) -> DaloResult<()> {
    if destination.is_dir() {
        let expected = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| state_error("artifact hash path is not valid UTF-8".to_owned()))?;
        if hash_tree(destination)? == expected {
            return Ok(());
        }
        return Err(state_error(format!(
            "content-addressed artifact `{}` failed its hash audit",
            destination.display()
        )));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| state_error("artifact path has no parent".to_owned()))?;
    fs::create_dir_all(parent)?;
    fs::rename(temporary.path(), destination)?;
    Ok(())
}

fn replace_owned_link(
    path: &Path,
    artifact: &Path,
    prior: Option<&ProjectionOwnership>,
) -> DaloResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::symlink_metadata(path).is_ok() {
        if !owned_link_matches(
            prior,
            path,
            prior.expect("checked ownership").artifact_path.as_path(),
        ) {
            return Err(state_error(format!(
                "refusing to replace foreign plugin path `{}`",
                path.display()
            )));
        }
        fs::remove_file(path)?;
    }
    let temporary = path.with_extension(format!("dalo-{}", std::process::id()));
    if fs::symlink_metadata(&temporary).is_ok() {
        fs::remove_file(&temporary)?;
    }
    unix_fs::symlink(artifact, &temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn remove_owned_link(entry: &ProjectionOwnership, dry_run: bool) -> DaloResult<()> {
    if fs::symlink_metadata(&entry.link_path).is_err() {
        return Ok(());
    }
    if !owned_link_matches(Some(entry), &entry.link_path, &entry.artifact_path) {
        return Err(state_error(format!(
            "owned plugin path `{}` drifted; refusing removal",
            entry.link_path.display()
        )));
    }
    if !dry_run {
        fs::remove_file(&entry.link_path)?;
    }
    Ok(())
}

fn snapshot_link(path: &Path) -> LinkSnapshot {
    LinkSnapshot {
        path: path.to_path_buf(),
        target: fs::read_link(path).ok(),
    }
}

fn restore_links(mutations: &[LinkMutation]) -> DaloResult<()> {
    for mutation in mutations.iter().rev() {
        match &mutation.expected_after {
            Some(expected) => {
                if !fs::read_link(&mutation.before.path).is_ok_and(|target| target == *expected) {
                    return Err(state_error(format!(
                        "plugin path `{}` changed concurrently after Dalo updated it",
                        mutation.before.path.display()
                    )));
                }
                fs::remove_file(&mutation.before.path)?;
            }
            None if fs::symlink_metadata(&mutation.before.path).is_ok() => {
                return Err(state_error(format!(
                    "plugin path `{}` changed concurrently after Dalo removed it",
                    mutation.before.path.display()
                )));
            }
            None => {}
        }
        if let Some(target) = &mutation.before.target {
            if let Some(parent) = mutation.before.path.parent() {
                fs::create_dir_all(parent)?;
            }
            unix_fs::symlink(target, &mutation.before.path)?;
        }
    }
    Ok(())
}

fn owned_link_matches(prior: Option<&ProjectionOwnership>, path: &Path, expected: &Path) -> bool {
    prior.is_some() && fs::read_link(path).is_ok_and(|target| target == expected)
}

fn read_state(paths: &StorePaths) -> DaloResult<ProjectionStateFile> {
    match fs::read(&paths.plugin_state_file) {
        Ok(bytes) => {
            let state: ProjectionStateFile = serde_json::from_slice(&bytes)?;
            if state.schema_version != PROJECTION_SCHEMA_VERSION {
                return Err(state_error(
                    "unsupported plugin projection state version".to_owned(),
                ));
            }
            Ok(state)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProjectionStateFile {
            schema_version: PROJECTION_SCHEMA_VERSION,
            entries: Vec::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

fn write_state(paths: &StorePaths, state: &ProjectionStateFile) -> DaloResult<()> {
    #[cfg(test)]
    if TEST_FAIL_STATE_WRITE.replace(false) {
        return Err(state_error(
            "injected plugin state write failure".to_owned(),
        ));
    }
    let parent = paths
        .plugin_state_file
        .parent()
        .ok_or_else(|| state_error("plugin state path has no parent".to_owned()))?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&paths.plugin_state_file)
        .map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn copy_package_tree(source: &Path, destination: &Path) -> DaloResult<()> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    let mut bytes = 0u64;
    while let Some((from, to, depth)) = pending.pop() {
        if depth > 32 {
            return Err(state_error(
                "component package exceeds 32 directory levels".to_owned(),
            ));
        }
        let metadata = fs::symlink_metadata(&from)?;
        if metadata.file_type().is_symlink() {
            return Err(state_error(format!(
                "component contains symlink `{}`",
                from.display()
            )));
        }
        entries += 1;
        if entries > MAX_COMPONENT_ENTRIES {
            return Err(state_error(
                "component package contains too many entries".to_owned(),
            ));
        }
        if metadata.is_dir() {
            fs::create_dir_all(&to)?;
            for entry in fs::read_dir(&from)? {
                let entry = entry?;
                pending.push((entry.path(), to.join(entry.file_name()), depth + 1));
            }
        } else if metadata.is_file() {
            bytes = bytes.saturating_add(metadata.len());
            if bytes > MAX_COMPONENT_BYTES {
                return Err(state_error(
                    "component package exceeds byte limit".to_owned(),
                ));
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        } else {
            return Err(state_error(format!(
                "component contains special file `{}`",
                from.display()
            )));
        }
    }
    Ok(())
}

fn hash_tree(root: &Path) -> DaloResult<String> {
    let mut files = BTreeMap::new();
    collect_hash_files(root, root, &mut files)?;
    let mut hash = Sha256::new();
    hash.update(b"dalo-native-plugin-v1\0");
    for (path, (executable, bytes)) in files {
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path.as_bytes());
        hash.update([u8::from(executable)]);
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(hex(hash.finalize().as_slice()))
}

fn collect_hash_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, (bool, Vec<u8>)>,
) -> DaloResult<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(state_error(
                "projected package unexpectedly contains a symlink".to_owned(),
            ));
        }
        if metadata.is_dir() {
            collect_hash_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| state_error("projection path escaped root".to_owned()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(
                relative,
                (metadata.permissions().mode() & 0o111 != 0, fs::read(path)?),
            );
        }
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> DaloResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}
fn write_json(path: &Path, value: &serde_json::Value) -> DaloResult<()> {
    write_file(path, &serde_json::to_vec_pretty(value)?)
}
fn hash_bytes(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn state_error(reason: String) -> DaloError {
    DaloError::StateError { reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserConfig;
    use crate::plugin::{
        MemberRequirement, PluginResolution, ResolvedPluginMember, SelectionOrigin,
        SelectionOriginKind, SelectionStrength,
    };
    use crate::store::TargetState;

    fn fixture(
        root: &Path,
    ) -> (
        StorePaths,
        StateFile,
        PluginResolution,
        Vec<SourceInventory>,
    ) {
        let source = root.join("source");
        let skill = source.join("skills/hello");
        let package = source.join("plugins/demo");
        fs::create_dir_all(&skill).expect("skill directory");
        fs::create_dir_all(&package).expect("plugin directory");
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: hello\ndescription: Hello\n---\n\nHello.\n",
        )
        .expect("skill file");
        fs::write(
            package.join("PLUGIN.toml"),
            "schema_version = 1\n\n[plugin]\nname = \"demo\"\ndescription = \"Demo plugin\"\nversion = \"1.2.3\"\n\n[[plugin.members]]\nref = \"skill:hello\"\nrequirement = \"required\"\n",
        )
        .expect("plugin manifest");
        let inventory = crate::inventory::scan_source("local", &source).expect("inventory");
        let record = inventory.plugins[0].clone();
        let plugin = ResolvedPlugin {
            source_ref: record.source_ref.clone(),
            slot_name: record.slot_name.clone(),
            id: record.id.clone(),
            source_priority: 0,
            package_hash: record.package_hash,
            closure_hash: "closure".to_owned(),
            origins: vec![SelectionOrigin {
                kind: SelectionOriginKind::Direct,
                declared_by: "user config".to_owned(),
                requirement: SelectionStrength::Required,
            }],
            policies: Vec::new(),
            state: PluginState::Selected,
            shadowed_by: None,
            members: vec![ResolvedPluginMember {
                reference: "skill:hello".to_owned(),
                requirement: MemberRequirement::Required,
                state: PluginComponentState::Active,
                resolved_ref: Some("local:hello".to_owned()),
                fallback: None,
            }],
            dependencies: Vec::new(),
            blocking_reasons: Vec::new(),
        };
        let paths = StorePaths::new(root.join("store"));
        fs::create_dir_all(&paths.root).expect("store");
        crate::store::write_config(&paths, &UserConfig::default_for_store(&paths.root))
            .expect("config");
        let mut state = StateFile::empty();
        for target in ["codex", "claude"] {
            let target_path = root.join(target).join("skills");
            fs::create_dir_all(&target_path).expect("target");
            state.targets.push(TargetState {
                id: target.to_owned(),
                path: target_path.clone(),
                canonical_path: target_path,
                enabled: true,
                extra: BTreeMap::new(),
            });
        }
        (
            paths,
            state,
            PluginResolution {
                plugins: vec![plugin],
                diagnostics: Vec::new(),
            },
            vec![inventory],
        )
    }

    #[test]
    fn projects_one_portable_plugin_to_valid_codex_and_claude_packages() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, state, plugins, inventories) = fixture(root.path());
        let reports =
            reconcile(&paths, &state, &plugins, &inventories, &[], &[], false).expect("projection");
        assert_eq!(reports.len(), 2);
        assert!(
            reports
                .iter()
                .all(|report| report.state == PluginProjectionState::Ready)
        );
        for report in &reports {
            let manifest = if report.target == "codex" {
                report.path.join(".codex-plugin/plugin.json")
            } else {
                report.path.join(".claude-plugin/plugin.json")
            };
            assert!(manifest.is_file());
            assert!(report.path.join("skills/hello/SKILL.md").is_file());
            assert!(report.path.join("dalo-provenance.json").is_file());
        }
        assert_eq!(
            fs::read_to_string(inventories[0].skills[0].path.join("SKILL.md")).expect("source"),
            "---\nname: hello\ndescription: Hello\n---\n\nHello.\n"
        );
    }

    #[test]
    fn dry_run_and_unselection_preserve_direct_skill_behavior() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, state, plugins, inventories) = fixture(root.path());
        let dry =
            reconcile(&paths, &state, &plugins, &inventories, &[], &[], true).expect("dry run");
        assert!(
            dry.iter()
                .all(|report| report.state == PluginProjectionState::Planned)
        );
        assert!(!paths.plugins_dir.exists());
        let installed =
            reconcile(&paths, &state, &plugins, &inventories, &[], &[], false).expect("install");
        let empty = PluginResolution::default();
        reconcile(&paths, &state, &empty, &inventories, &[], &[], false).expect("uninstall");
        assert!(installed.iter().all(|report| !report.path.exists()));
        assert!(inventories[0].skills[0].path.join("SKILL.md").is_file());
    }

    #[test]
    fn refuses_to_remove_a_drifted_owned_projection() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, state, plugins, inventories) = fixture(root.path());
        let installed =
            reconcile(&paths, &state, &plugins, &inventories, &[], &[], false).expect("install");
        let path = installed[0].path.clone();
        fs::remove_file(&path).expect("remove owned link");
        fs::write(&path, "foreign").expect("foreign replacement");
        let error = reconcile(
            &paths,
            &state,
            &PluginResolution::default(),
            &inventories,
            &[],
            &[],
            false,
        )
        .expect_err("drift must block removal");
        assert!(error.to_string().contains("drifted"));
        assert_eq!(
            fs::read_to_string(path).expect("foreign content"),
            "foreign"
        );
    }

    #[test]
    fn updates_and_disables_each_provider_projection_independently() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, mut state, plugins, mut inventories) = fixture(root.path());
        let first = reconcile(&paths, &state, &plugins, &inventories, &[], &[], false)
            .expect("first projection");
        let old_hashes = first
            .iter()
            .map(|report| (report.target.clone(), report.projection_hash.clone()))
            .collect::<BTreeMap<_, _>>();
        fs::write(
            inventories[0].skills[0].path.join("SKILL.md"),
            "---\nname: hello\ndescription: Hello\n---\n\nUpdated.\n",
        )
        .expect("updated skill");
        inventories = vec![
            crate::inventory::scan_source("local", &root.path().join("source"))
                .expect("updated inventory"),
        ];
        let updated = reconcile(&paths, &state, &plugins, &inventories, &[], &[], false)
            .expect("updated projection");
        assert!(updated.iter().all(|report| {
            old_hashes.get(&report.target) != Some(&report.projection_hash)
                && fs::read_to_string(report.path.join("skills/hello/SKILL.md"))
                    .is_ok_and(|bytes| bytes.contains("Updated"))
        }));

        state
            .targets
            .iter_mut()
            .find(|target| target.id == "claude")
            .expect("Claude target")
            .enabled = false;
        reconcile(&paths, &state, &plugins, &inventories, &[], &[], false).expect("disable Claude");
        let claude = updated
            .iter()
            .find(|report| report.target == "claude")
            .expect("Claude report");
        let codex = updated
            .iter()
            .find(|report| report.target == "codex")
            .expect("Codex report");
        assert!(!claude.path.exists());
        assert!(codex.path.exists());
    }

    #[test]
    fn rolls_back_all_provider_links_when_state_commit_fails() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, state, plugins, mut inventories) = fixture(root.path());
        let installed = reconcile(&paths, &state, &plugins, &inventories, &[], &[], false)
            .expect("initial projection");
        let original_targets = installed
            .iter()
            .map(|report| {
                (
                    report.path.clone(),
                    fs::read_link(&report.path).expect("owned link"),
                )
            })
            .collect::<Vec<_>>();
        fs::write(
            inventories[0].skills[0].path.join("SKILL.md"),
            "---\nname: hello\ndescription: Hello\n---\n\nChanged.\n",
        )
        .expect("changed skill");
        inventories = vec![
            crate::inventory::scan_source("local", &root.path().join("source"))
                .expect("changed inventory"),
        ];
        TEST_FAIL_STATE_WRITE.set(true);
        let error = reconcile(&paths, &state, &plugins, &inventories, &[], &[], false)
            .expect_err("state failure");
        assert!(error.to_string().contains("injected"));
        for (path, target) in original_targets {
            assert_eq!(fs::read_link(path).expect("rolled back link"), target);
        }
    }

    #[test]
    fn detects_drift_inside_an_immutable_artifact_before_reuse() {
        let root = tempfile::tempdir().expect("temp root");
        let (paths, state, plugins, inventories) = fixture(root.path());
        let installed = reconcile(&paths, &state, &plugins, &inventories, &[], &[], false)
            .expect("initial projection");
        let codex = installed
            .iter()
            .find(|report| report.target == "codex")
            .expect("Codex projection");
        fs::write(codex.artifact_path.join("skills/hello/SKILL.md"), "drifted")
            .expect("artifact drift");
        let reports = reconcile(&paths, &state, &plugins, &inventories, &[], &[], true)
            .expect("drift report");
        assert_eq!(
            reports
                .iter()
                .find(|report| report.target == "codex")
                .expect("Codex report")
                .state,
            PluginProjectionState::Conflict
        );
        assert!(codex.path.exists());
    }

    #[test]
    fn native_names_are_kebab_case_and_collision_resistant() {
        let dotted = native_name("company.marketing:review");
        let dashed = native_name("company-marketing:review");
        assert!(
            dotted
                .bytes()
                .all(|byte| { byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' })
        );
        assert_ne!(dotted, dashed);
        assert!(dotted.len() <= 80);
    }
}
