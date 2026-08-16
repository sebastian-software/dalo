//! Target-aware hook sidecar planning and reconciliation.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::error::{DaloError, DaloResult};
use crate::hook::{
    HookFallback, HookProvider, HookRequirement, HookStatusReport, HookTrustState,
    NativeHookProjection, compile_native_projection,
};
use crate::hook_sidecar::{self, HookSidecarAction, HookSidecarPlan};
use crate::store::{StateFile, StorePaths};

/// One provider hook reconciliation result retained by sync/status JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookTargetReport {
    /// Logical provider target.
    pub target: String,
    /// Native sidecar path.
    pub path: PathBuf,
    /// Exact observed provider version, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    /// Stable target result.
    pub state: HookTargetState,
    /// Planned or completed sidecar action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<HookSidecarAction>,
    /// Number of ready portable hooks in the desired native projection.
    pub projected_hooks: usize,
    /// Whether no mutation was attempted.
    pub dry_run: bool,
    /// Actionable explanation.
    pub diagnostic: String,
}

/// Distinct provider hook states required by #501.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTargetState {
    /// Exact desired sidecar already exists.
    Ready,
    /// Dry-run operation is ready to apply.
    Planned,
    /// Provider hooks are disabled by user or managed configuration.
    Disabled,
    /// Codex admits managed hooks only.
    ManagedOnly,
    /// Provider executable is absent.
    RuntimeMissing,
    /// Provider version differs from the executable fixture baseline.
    UnverifiedVersion,
    /// Required hook trust, tool readiness, or adapter semantics are unavailable.
    Blocked,
    /// Native content or ownership state conflicts with Dalo's exact prior entry.
    Conflict,
}

#[derive(Debug, Clone)]
struct ProviderFacts {
    target: &'static str,
    provider: HookProvider,
    sidecar: PathBuf,
    version: Option<String>,
    runtime_available: bool,
    hooks_enabled: bool,
    plugin_hooks_allowed: bool,
}

/// Reconcile selected plugin hooks independently for every linked native target.
pub fn reconcile(
    paths: &StorePaths,
    state: &StateFile,
    selected_plugins: &[String],
    dry_run: bool,
) -> DaloResult<Vec<HookTargetReport>> {
    let executable = env::current_exe()?;
    let hooks = crate::hook::list(paths)?.hooks;
    let mut reports = Vec::new();
    for target in state.targets.iter().filter(|target| target.enabled) {
        if !matches!(target.id.as_str(), "codex" | "claude") {
            continue;
        }
        let facts = provider_facts(&target.id)?;
        let selected = hooks
            .iter()
            .filter(|status| {
                selected_plugins.iter().any(|plugin| {
                    status
                        .hook
                        .source_ref
                        .starts_with(&format!("{plugin}#hook:"))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        reports.push(reconcile_target(
            paths,
            &facts,
            &executable,
            &selected,
            dry_run,
        ));
    }
    reports.sort_by(|left, right| left.target.cmp(&right.target));
    Ok(reports)
}

fn reconcile_target(
    paths: &StorePaths,
    facts: &ProviderFacts,
    executable: &Path,
    selected: &[HookStatusReport],
    dry_run: bool,
) -> HookTargetReport {
    let unavailable_required = selected.iter().find(|status| {
        status.hook.descriptor.requirement == HookRequirement::Required
            && status.state != HookTrustState::Ready
    });
    let ready = if facts.hooks_enabled && facts.plugin_hooks_allowed {
        selected
            .iter()
            .filter(|status| status.state == HookTrustState::Ready)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if ready.is_empty() {
        let empty = compile_native_projection(
            paths,
            facts.provider,
            facts.provider.baseline(),
            executable,
            &[],
        )
        .expect("empty baseline projection is always representable");
        let action = match reconcile_projection(paths, facts, &empty, dry_run) {
            Ok(action) => Some(action),
            Err(error) => {
                return report(facts, HookTargetState::Conflict, None, 0, dry_run, &error);
            }
        };
        if !facts.hooks_enabled {
            return report(
                facts,
                HookTargetState::Disabled,
                action,
                0,
                dry_run,
                "provider hooks are disabled; Dalo-owned entries are removed",
            );
        }
        if !facts.plugin_hooks_allowed {
            return report(
                facts,
                HookTargetState::ManagedOnly,
                action,
                0,
                dry_run,
                "provider policy admits managed hooks only; Dalo-owned entries are removed",
            );
        }
        if let Some(status) = unavailable_required {
            return report(
                facts,
                HookTargetState::Blocked,
                action,
                0,
                dry_run,
                &format!(
                    "required hook `{}`: {}",
                    status.hook.source_ref, status.diagnostic
                ),
            );
        }
        return report(
            facts,
            if dry_run && action != Some(HookSidecarAction::Noop) {
                HookTargetState::Planned
            } else {
                HookTargetState::Ready
            },
            action,
            0,
            dry_run,
            if selected.is_empty() {
                "no selected portable hooks; prior owned entries are removed"
            } else {
                "unavailable optional hooks are explicitly omitted"
            },
        );
    }
    if !facts.runtime_available {
        let action = match remove_owned_projection(paths, facts, executable, dry_run) {
            Ok(action) => Some(action),
            Err(error) => {
                return report(facts, HookTargetState::Conflict, None, 0, dry_run, &error);
            }
        };
        return report(
            facts,
            HookTargetState::RuntimeMissing,
            action,
            0,
            dry_run,
            "provider executable is absent from PATH",
        );
    }
    let Some(version) = facts.version.as_deref() else {
        let action = match remove_owned_projection(paths, facts, executable, dry_run) {
            Ok(action) => Some(action),
            Err(error) => {
                return report(facts, HookTargetState::Conflict, None, 0, dry_run, &error);
            }
        };
        return report(
            facts,
            HookTargetState::UnverifiedVersion,
            action,
            0,
            dry_run,
            "provider version could not be determined",
        );
    };
    if version != facts.provider.baseline() {
        let action = match remove_owned_projection(paths, facts, executable, dry_run) {
            Ok(action) => Some(action),
            Err(error) => {
                return report(facts, HookTargetState::Conflict, None, 0, dry_run, &error);
            }
        };
        return report(
            facts,
            HookTargetState::UnverifiedVersion,
            action,
            0,
            dry_run,
            &format!(
                "provider version `{version}` differs from verified baseline `{}`",
                facts.provider.baseline()
            ),
        );
    }
    let projection =
        match compile_native_projection(paths, facts.provider, version, executable, &ready) {
            Ok(projection) => projection,
            Err(error) => {
                return report(
                    facts,
                    HookTargetState::Blocked,
                    None,
                    0,
                    dry_run,
                    &error.to_string(),
                );
            }
        };
    let action = match reconcile_projection(paths, facts, &projection, dry_run) {
        Ok(action) => Some(action),
        Err(error) => {
            return report(
                facts,
                HookTargetState::Conflict,
                None,
                ready.len(),
                dry_run,
                &error,
            );
        }
    };
    if let Some(status) = unavailable_required {
        return report(
            facts,
            HookTargetState::Blocked,
            action,
            ready.len(),
            dry_run,
            &format!(
                "required hook `{}`: {}",
                status.hook.source_ref, status.diagnostic
            ),
        );
    }
    let omitted_optional = selected.iter().any(|status| {
        status.hook.descriptor.requirement == HookRequirement::Optional
            && status.state != HookTrustState::Ready
            && status.hook.descriptor.fallback == Some(HookFallback::Omit)
    });
    report(
        facts,
        if dry_run && action != Some(HookSidecarAction::Noop) {
            HookTargetState::Planned
        } else {
            HookTargetState::Ready
        },
        action,
        ready.len(),
        dry_run,
        if omitted_optional {
            "ready hooks projected; unavailable optional hooks explicitly omitted"
        } else {
            "native hook sidecar matches the approved portable projection"
        },
    )
}

fn reconcile_projection(
    paths: &StorePaths,
    facts: &ProviderFacts,
    projection: &NativeHookProjection,
    dry_run: bool,
) -> Result<HookSidecarAction, String> {
    let plan = hook_sidecar::plan_sidecar(paths, facts.provider, &facts.sidecar, projection)
        .map_err(|error| error.to_string())?;
    let action = plan.action;
    apply(paths, projection, plan, dry_run).map_err(|error| error.to_string())?;
    Ok(action)
}

fn remove_owned_projection(
    paths: &StorePaths,
    facts: &ProviderFacts,
    executable: &Path,
    dry_run: bool,
) -> Result<HookSidecarAction, String> {
    let empty = compile_native_projection(
        paths,
        facts.provider,
        facts.provider.baseline(),
        executable,
        &[],
    )
    .expect("empty baseline projection is always representable");
    reconcile_projection(paths, facts, &empty, dry_run)
}

fn apply(
    paths: &StorePaths,
    projection: &NativeHookProjection,
    plan: HookSidecarPlan,
    dry_run: bool,
) -> DaloResult<()> {
    hook_sidecar::apply_sidecar(paths, projection, plan, dry_run).map(|_| ())
}

fn report(
    facts: &ProviderFacts,
    state: HookTargetState,
    action: Option<HookSidecarAction>,
    projected_hooks: usize,
    dry_run: bool,
    diagnostic: &str,
) -> HookTargetReport {
    HookTargetReport {
        target: facts.target.to_owned(),
        path: facts.sidecar.clone(),
        provider_version: facts.version.clone(),
        state,
        action,
        projected_hooks,
        dry_run,
        diagnostic: diagnostic.to_owned(),
    }
}

fn provider_facts(target: &str) -> DaloResult<ProviderFacts> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| DaloError::StateError {
            reason: "HOME is required to resolve native provider hook files".to_owned(),
        })?;
    match target {
        "codex" => {
            let root = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            let config = read_toml(&root.join("config.toml"));
            let hooks_enabled = config
                .as_ref()
                .and_then(|value| value.get("features"))
                .and_then(|value| value.get("hooks"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(true);
            let managed_only = config
                .as_ref()
                .and_then(|value| value.get("allow_managed_hooks_only"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            let (runtime_available, version) = provider_version("codex", "codex-cli ");
            Ok(ProviderFacts {
                target: "codex",
                provider: HookProvider::Codex,
                sidecar: root.join("hooks.json"),
                version,
                runtime_available,
                hooks_enabled,
                plugin_hooks_allowed: !managed_only,
            })
        }
        "claude" => {
            let root = env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude"));
            let settings = read_json(&root.join("settings.json"));
            let hooks_enabled = settings
                .as_ref()
                .and_then(|value| value.get("disableAllHooks"))
                .and_then(serde_json::Value::as_bool)
                != Some(true);
            let (runtime_available, version) = provider_version("claude", "");
            Ok(ProviderFacts {
                target: "claude",
                provider: HookProvider::Claude,
                sidecar: root.join("settings.json"),
                version,
                runtime_available,
                hooks_enabled,
                plugin_hooks_allowed: true,
            })
        }
        _ => unreachable!("caller filters native hook targets"),
    }
}

fn provider_version(program: &str, prefix: &str) -> (bool, Option<String>) {
    let available = executable_on_path(program);
    if !available {
        return (false, None);
    }
    let version = Command::new(program)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let bytes = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            String::from_utf8(bytes).ok()
        })
        .and_then(|output| {
            output
                .trim()
                .strip_prefix(prefix)
                .unwrap_or(output.trim())
                .split_whitespace()
                .next()
                .map(str::to_owned)
        });
    (true, version)
}

fn executable_on_path(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| fs_metadata_file(&directory.join(program)))
    })
}

fn fs_metadata_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    std::fs::read(path)
        .ok()
        .and_then(|content| serde_json::from_slice(&content).ok())
}
