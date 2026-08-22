//! Diagnostics for store, target, Git, and lockfile health.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::adopt;
use crate::audit;
use crate::autosync::{self, AutosyncRunOutcome};
use crate::catalog::{self, SourceLock};
use crate::config::UserConfig;
use crate::error::shell_quote_path;
use crate::git;
use crate::instructions;
use crate::inventory::{InventoryWarning, InventoryWarningCode, SourceInventory};
use crate::plan::InstallationPlan;
use crate::resolver;
use crate::source::{self, SourceConfig, SourceKind};
use crate::store::{self, ApprovalsFile, OwnedSkillState, StateFile, StorePaths};

const COMMAND_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_CHECK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Doctor report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    /// Store root.
    pub store: PathBuf,
    /// Diagnostic findings.
    pub findings: Vec<DoctorFinding>,
    /// Summary counts by severity.
    pub summary: DoctorSummary,
    /// Read-only provider-aware skill delivery selections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deliveries: Vec<crate::materialize::SkillDeliveryReport>,
    /// Shared typed planning facts when plugins are selected and inputs parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation_plan: Option<InstallationPlan>,
}

/// Count of findings by severity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DoctorSummary {
    /// Error count.
    pub errors: usize,
    /// Warning count.
    pub warnings: usize,
    /// Info count.
    pub info: usize,
    /// OK count.
    pub ok: usize,
}

/// One diagnostic finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorFinding {
    /// Severity.
    pub severity: DoctorSeverity,
    /// Machine-readable code.
    pub code: DoctorCode,
    /// Human-readable message.
    pub message: String,
    /// Suggested next command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
    /// Structured source-inventory warnings for degraded-inventory findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inventory_warnings: Vec<InventoryWarning>,
}

/// Diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    /// Blocks normal operation.
    Error,
    /// May block a subset of workflows or deserves attention.
    Warning,
    /// Useful context.
    Info,
    /// Check passed.
    Ok,
}

/// Diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorCode {
    /// Store root exists.
    StoreExists,
    /// Store root is missing.
    StoreMissing,
    /// Expected store layout exists.
    StoreLayoutOk,
    /// Expected store path is missing.
    StoreLayoutMissing,
    /// Config parses.
    ConfigOk,
    /// Config cannot be parsed.
    ConfigInvalid,
    /// State parses.
    StateOk,
    /// State cannot be parsed.
    StateInvalid,
    /// Lock parses.
    LockOk,
    /// Lock cannot be parsed.
    LockInvalid,
    /// Catalog source lock is present and parses.
    SourceLockOk,
    /// Catalog source lock cannot be parsed.
    SourceLockInvalid,
    /// Approvals file parses.
    ApprovalsOk,
    /// Approvals file cannot be parsed.
    ApprovalsInvalid,
    /// Git executable is available.
    GitAvailable,
    /// Git executable is missing.
    GitMissing,
    /// Local source has a Git repository.
    LocalGitOk,
    /// Local source Git repository is missing.
    LocalGitMissing,
    /// Configured target exists.
    TargetExists,
    /// Configured target is missing.
    TargetMissing,
    /// Multiple logical targets share a directory.
    DuplicateTargetDirectory,
    /// Owned symlink is valid.
    OwnedSymlinkOk,
    /// Owned symlink is missing.
    MissingOwnedSymlink,
    /// Owned symlink points to a missing store path.
    BrokenOwnedSymlink,
    /// Recorded owned slot is a real entry.
    OwnedPathRealEntry,
    /// Recorded owned symlink points outside the store.
    ForeignOwnedSymlink,
    /// Recorded owned symlink points to a different path inside the store.
    OwnedSymlinkRepointed,
    /// Unmanaged target skill blocks the same managed slot.
    UnmanagedSameNameBlocker,
    /// A protected target slot is intentionally kept unmanaged.
    ProtectedSkillKept,
    /// A protection record no longer maps to an existing target slot.
    StaleProtectedSkill,
    /// A linked target directory could not be scanned for unmanaged skills.
    UnreadableTargetDirectory,
    /// Source is clean.
    SourceClean,
    /// Source has local changes.
    DirtySource,
    /// An enabled source's checkout is missing from disk.
    SourceMissing,
    /// A source inventory is partial, so sync preserves existing links.
    SourceInventoryDegraded,
    /// Manifest-derived source provenance is internally consistent.
    SourceProvenanceOk,
    /// Manifest declaration, checkout, or source lock disagree.
    SourceProvenanceMismatch,
    /// Store contains checkout or staging content not owned by config.
    SourceStoreDebris,
    /// A skill is pending approval.
    PendingApproval,
    /// A skill is blocked because its required closure is not linkable.
    RequiredClosureBlocked,
    /// An active skill is blocked by an unaccepted security-audit finding.
    SecurityAuditBlocked,
    /// An active skill's deterministic security audit could not be completed.
    SecurityAuditFailed,
    /// A local tool is awaiting an exact executable-contract approval.
    ToolPendingApproval,
    /// A previously approved local-tool contract changed.
    ToolHashDrift,
    /// A local tool's required runtime is absent.
    ToolRuntimeMissing,
    /// A local tool excludes the current platform.
    ToolPlatformMismatch,
    /// A local tool approval was revoked while immutable bytes remain.
    ToolApprovalRevoked,
    /// A local tool's immutable staged closure failed verification.
    ToolAuditFailed,
    /// A local tool is exactly approved and immutably staged.
    ToolReady,
    /// Interrupted tool staging debris is safely outside promoted hashes.
    ToolStagingDebris,
    /// A generated delivery cache or read-only planning pass failed verification.
    GeneratedDeliveryInvalid,
    /// Interrupted generated-delivery staging debris was never promoted.
    GeneratedDeliveryStagingDebris,
    /// A hook awaits its independent exact contract approval.
    HookPendingApproval,
    /// A hook or its referenced tool contract changed.
    HookHashDrift,
    /// A hook's separately approved tool is unavailable.
    HookToolUnavailable,
    /// A hook and its referenced tool are independently ready.
    HookReady,
    /// A native provider disabled or excludes plugin hook projection.
    HookProviderDisabled,
    /// A native provider version/runtime is unavailable or unverified.
    HookProviderUnverified,
    /// Native sidecar content conflicts with Dalo ownership state.
    HookNativeConflict,
    /// A selected portable plugin cannot produce a coherent native package.
    PluginProjectionBlocked,
    /// A native plugin path or ownership record has drifted.
    PluginProjectionConflict,
    /// Two active instruction packs declare overlapping topics.
    InstructionPackTopicOverlap,
    /// An active instruction pack's rendered block is missing, malformed, or stale.
    InstructionBlockDrift,
    /// Target path looks cloud-synced.
    CloudSyncedTarget,
    /// Scheduled synchronization is installed and enabled.
    AutosyncInstalled,
    /// Scheduled synchronization is not installed.
    AutosyncNotInstalled,
    /// Scheduler metadata exists but the native job is disabled.
    AutosyncDisabled,
    /// The executable recorded in scheduler metadata is unavailable.
    AutosyncExecutableMissing,
    /// The latest scheduled synchronization was blocked.
    AutosyncRunBlocked,
    /// A scheduled run is still marked `running` long after it started.
    AutosyncRunStale,
    /// Autosync metadata or native scheduler state could not be inspected.
    AutosyncStateInvalid,
}

/// Run read-only diagnostics.
pub fn run_doctor(store_root: &Path) -> DoctorReport {
    let paths = StorePaths::new(store_root.to_path_buf());
    let mut findings = Vec::new();

    check_store_layout(&paths, &mut findings);
    if !paths.root.is_dir() {
        return finish_report(store_root, findings, None, Vec::new());
    }
    check_commands(&mut findings);

    let config = read_config(&paths, &mut findings);
    let state = read_state(&paths, &mut findings);
    let lock = read_lock(&paths, &mut findings);
    let source_lock = read_source_lock(&paths, &mut findings);
    let approvals = read_approvals(&paths, &mut findings);

    if paths.local_dir.join(".git").is_dir() {
        findings.push(ok(
            DoctorCode::LocalGitOk,
            "local source Git repository exists",
        ));
    } else if paths.root.exists() {
        findings.push(finding_error(
            DoctorCode::LocalGitMissing,
            "local source Git repository is missing",
            Some("dalo init".to_owned()),
        ));
    }

    if let Some(state) = state.as_ref() {
        check_targets(state, &mut findings);
        check_owned_symlinks(&paths, state, &mut findings);
        check_protected_skills(state, &mut findings);
    }

    let mut plugin_inventories = None;
    let mut reconciliation_inventories = None;
    let mut live_resolution = config.as_ref().map(|config| {
        let approval_records = approvals
            .as_ref()
            .map(|approvals| approvals.approvals.clone())
            .unwrap_or_default();
        let resolved =
            resolver::resolve_from_config_with_plugin_inventories(config, approval_records);
        plugin_inventories = Some(resolver::plugin_inventories(&resolved));
        reconciliation_inventories = Some(resolver::inventories_with_plugins(&resolved));
        resolved.live
    });
    if let (Some(live), Some(lock)) = (live_resolution.as_mut(), lock.as_ref()) {
        let active_instructions = lock
            .active_instruction_packs
            .iter()
            .map(|pack| format!("{}:{}", pack.source_id, pack.pack_id))
            .collect::<BTreeSet<_>>();
        crate::plugin::apply_component_resolution(
            &mut live.plugins,
            &live.resolution,
            &live.agents,
            &active_instructions,
        );
    }

    if let Some(config) = config.as_ref() {
        check_sources(config, &source_lock, &mut findings);
        check_source_inventories(live_resolution.as_ref(), &mut findings);
        check_source_store_debris(&paths, config, &mut findings);
    }
    let tool_report = match (
        config.as_ref(),
        approvals.as_ref(),
        plugin_inventories.as_ref(),
    ) {
        (Some(config), Some(approvals), Some(inventories)) => {
            Some(crate::tool::list_from_inventories(
                &paths,
                &config.sources,
                &approvals.approvals,
                inventories,
            ))
        }
        _ => None,
    };
    let hook_report = match (
        config.as_ref(),
        approvals.as_ref(),
        plugin_inventories.as_ref(),
        tool_report.as_ref(),
    ) {
        (Some(config), Some(approvals), Some(inventories), Some(tools)) => {
            crate::hook::list_from_inventories(
                &paths,
                &config.sources,
                &approvals.approvals,
                inventories,
                &tools.tools,
            )
            .ok()
        }
        _ => None,
    };
    check_tools(&paths, tool_report.as_ref(), &mut findings);
    check_generated_delivery_staging(&paths, &mut findings);
    check_hooks(hook_report.as_ref(), &mut findings);

    // A corrupt lock is reported by `read_lock`, but resolution/instruction/
    // blocker checks do not depend on it (they re-derive from config/state and
    // read the user lock via `unwrap_or_default`), so they must not be gated on
    // a valid lock.
    if let (Some(config), Some(_), Some(live_resolution)) =
        (config.as_ref(), state.as_ref(), live_resolution.as_ref())
    {
        check_resolution(
            &paths,
            config,
            live_resolution,
            approvals.is_some(),
            &mut findings,
        );
    }
    if let (Some(state), Some(live), Some(inventories)) = (
        state.as_ref(),
        live_resolution.as_ref(),
        reconciliation_inventories.as_deref(),
    ) {
        check_hook_targets(&paths, state, live, hook_report.as_ref(), &mut findings);
        check_plugin_targets(
            &paths,
            state,
            live,
            inventories,
            tool_report.as_ref(),
            hook_report.as_ref(),
            &mut findings,
        );
    }
    check_autosync(&paths, &mut findings);

    let materialization = live_resolution.as_ref().and_then(|live| {
        match crate::materialize::materialize(&paths, &live.resolution, true) {
            Ok(report) => Some(report),
            Err(error) => {
                findings.push(finding_error(
                    DoctorCode::GeneratedDeliveryInvalid,
                    format!("delivery planning or generated cache verification failed: {error}"),
                    None,
                ));
                None
            }
        }
    });
    let installation_plan = match (
        state.as_ref(),
        live_resolution.as_ref(),
        materialization.as_ref(),
        reconciliation_inventories.as_deref(),
    ) {
        (Some(state), Some(live), Some(materialization), Some(inventories))
            if !live.plugins.plugins.is_empty() =>
        {
            let mut plan = crate::plan::build_from_facts(
                store_root,
                state,
                &live.plugins,
                inventories,
                &materialization.operations,
                None,
            );
            if let Some(tools) = tool_report.as_ref() {
                crate::plan::attach_tool_status_from_report(&mut plan, &tools.tools);
            }
            if let Some(hooks) = hook_report.as_ref() {
                crate::plan::attach_hook_status_from_report(&mut plan, &hooks.hooks);
            }
            Some(plan)
        }
        _ => None,
    };
    let deliveries = materialization.map_or_else(Vec::new, |report| report.deliveries);
    finish_report(store_root, findings, installation_plan, deliveries)
}

fn check_plugin_targets(
    paths: &StorePaths,
    state: &crate::store::StateFile,
    live: &crate::resolver::LiveResolution,
    inventories: &[SourceInventory],
    tool_report: Option<&crate::tool::ToolListReport>,
    hook_report: Option<&crate::hook::HookListReport>,
    findings: &mut Vec<DoctorFinding>,
) {
    let tools = tool_report.map_or_else(Vec::new, |report| report.tools.clone());
    let hooks = hook_report.map_or_else(Vec::new, |report| report.hooks.clone());
    let reports = match crate::plugin_projection::reconcile(
        paths,
        state,
        &live.plugins,
        inventories,
        &tools,
        &hooks,
        true,
    ) {
        Ok(reports) => reports,
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::PluginProjectionConflict,
                format!("native plugin projection: {error}"),
                Some(
                    "review `dalo status` and restore or remove the drifted native plugin path"
                        .to_owned(),
                ),
            ));
            return;
        }
    };
    for report in reports {
        match report.state {
            crate::plugin_projection::PluginProjectionState::Ready
            | crate::plugin_projection::PluginProjectionState::Planned => {}
            crate::plugin_projection::PluginProjectionState::Blocked => {
                findings.push(finding_error(
                    DoctorCode::PluginProjectionBlocked,
                    format!(
                        "{} plugin `{}`: {}",
                        report.target, report.plugin, report.diagnostic
                    ),
                    Some("run `dalo plan` and resolve the required component blocker".to_owned()),
                ));
            }
            crate::plugin_projection::PluginProjectionState::Conflict => {
                findings.push(finding_error(
                    DoctorCode::PluginProjectionConflict,
                    format!("{} plugin `{}`: {}", report.target, report.plugin, report.diagnostic),
                    Some("restore the Dalo-owned link or move foreign provider content out of the target path".to_owned()),
                ));
            }
        }
    }
}

fn check_hooks(report: Option<&crate::hook::HookListReport>, findings: &mut Vec<DoctorFinding>) {
    let Some(report) = report else {
        return;
    };
    for hook in &report.hooks {
        let identity = &hook.hook.source_ref;
        match hook.state {
            crate::hook::HookTrustState::Ready => findings.push(ok(
                DoctorCode::HookReady,
                format!("hook `{identity}` and its referenced tool are exactly approved"),
            )),
            crate::hook::HookTrustState::PendingApproval => findings.push(finding_warning(
                DoctorCode::HookPendingApproval,
                format!("hook `{identity}` is pending independent approval"),
                Some(format!("dalo approve hook {identity}")),
            )),
            crate::hook::HookTrustState::HashDrift => findings.push(finding_error(
                DoctorCode::HookHashDrift,
                format!("hook `{identity}` changed its security-relevant contract"),
                Some(format!("dalo hook show {identity}")),
            )),
            crate::hook::HookTrustState::ToolUnavailable => findings.push(finding_error(
                DoctorCode::HookToolUnavailable,
                format!("hook `{identity}`: {}", hook.diagnostic),
                Some(format!("dalo approve tool {}", hook.hook.tool_source_ref)),
            )),
        }
    }
}

fn check_hook_targets(
    paths: &StorePaths,
    state: &crate::store::StateFile,
    live: &crate::resolver::LiveResolution,
    hook_report: Option<&crate::hook::HookListReport>,
    findings: &mut Vec<DoctorFinding>,
) {
    let selected = live
        .plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.state == crate::plugin::PluginState::Selected)
        .map(|plugin| plugin.source_ref.clone())
        .collect::<Vec<_>>();
    let Some(hooks) = hook_report else {
        return;
    };
    let Ok(reports) =
        crate::hook_sync::reconcile_with_hooks(paths, state, &selected, &hooks.hooks, true)
    else {
        return;
    };
    for report in reports {
        use crate::hook_sync::HookTargetState;
        match report.state {
            HookTargetState::Ready | HookTargetState::Planned => {}
            HookTargetState::Disabled | HookTargetState::ManagedOnly => {
                findings.push(finding_warning(
                    DoctorCode::HookProviderDisabled,
                    format!("{} hooks: {}", report.target, report.diagnostic),
                    None,
                ));
            }
            HookTargetState::RuntimeMissing | HookTargetState::UnverifiedVersion => {
                findings.push(finding_error(
                    DoctorCode::HookProviderUnverified,
                    format!("{} hooks: {}", report.target, report.diagnostic),
                    None,
                ));
            }
            HookTargetState::Conflict => findings.push(finding_error(
                DoctorCode::HookNativeConflict,
                format!("{} hooks: {}", report.target, report.diagnostic),
                None,
            )),
            HookTargetState::Blocked => findings.push(finding_error(
                DoctorCode::HookPendingApproval,
                format!("{} hooks: {}", report.target, report.diagnostic),
                None,
            )),
        }
    }
}

fn check_tools(
    paths: &StorePaths,
    report: Option<&crate::tool::ToolListReport>,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(report) = report else {
        return;
    };
    for tool in &report.tools {
        use crate::tool::ToolState;
        let identity = &tool.tool.source_ref;
        match tool.state {
            ToolState::Ready => findings.push(ok(
                DoctorCode::ToolReady,
                format!("tool `{identity}` is exactly approved and immutably staged"),
            )),
            ToolState::PendingApproval => findings.push(finding_warning(
                DoctorCode::ToolPendingApproval,
                format!("tool `{identity}` is pending exact execution approval"),
                Some(format!("dalo approve tool {identity}")),
            )),
            ToolState::HashDrift => findings.push(finding_error(
                DoctorCode::ToolHashDrift,
                format!("tool `{identity}` changed its executable contract"),
                Some(format!("dalo tool audit {identity}")),
            )),
            ToolState::RuntimeMissing => findings.push(finding_error(
                DoctorCode::ToolRuntimeMissing,
                format!("tool `{identity}`: {}", tool.diagnostic),
                None,
            )),
            ToolState::PlatformMismatch => findings.push(finding_warning(
                DoctorCode::ToolPlatformMismatch,
                format!("tool `{identity}`: {}", tool.diagnostic),
                None,
            )),
            ToolState::Revoked => findings.push(finding_warning(
                DoctorCode::ToolApprovalRevoked,
                format!("tool `{identity}` no longer has execution approval"),
                Some(format!("dalo approve tool {identity}")),
            )),
            ToolState::AuditFailure => findings.push(finding_error(
                DoctorCode::ToolAuditFailed,
                format!("tool `{identity}` failed immutable closure verification"),
                Some(format!("dalo tool audit {identity}")),
            )),
            ToolState::ApprovedNotStaged => findings.push(finding_error(
                DoctorCode::ToolAuditFailed,
                format!("tool `{identity}` is approved but immutable staging is incomplete"),
                Some(format!("dalo approve tool {identity}")),
            )),
        }
    }
    let staging_parent = paths.tools_dir.join("sha256");
    if let Ok(entries) = fs::read_dir(&staging_parent) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".tool-stage-"))
            {
                findings.push(finding_warning(
                    DoctorCode::ToolStagingDebris,
                    format!(
                        "interrupted tool staging debris exists at `{}`; it was never promoted or approved",
                        entry.path().display()
                    ),
                    None,
                ));
            }
        }
    }
}

fn check_generated_delivery_staging(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) {
    let parent = paths.generated_dir.join("sha256");
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".delivery-stage-"))
        {
            findings.push(finding_warning(
                DoctorCode::GeneratedDeliveryStagingDebris,
                format!(
                    "interrupted generated-delivery staging debris exists at `{}`; it was never promoted or activated",
                    entry.path().display()
                ),
                None,
            ));
        }
    }
}

fn finish_report(
    store_root: &Path,
    mut findings: Vec<DoctorFinding>,
    installation_plan: Option<InstallationPlan>,
    deliveries: Vec<crate::materialize::SkillDeliveryReport>,
) -> DoctorReport {
    for finding in &mut findings {
        finding.message = store::contextualize_dalo_commands(store_root, &finding.message);
        if let Some(next_command) = &mut finding.next_command {
            *next_command = store::contextualize_dalo_commands(store_root, next_command);
        }
    }
    findings.sort_by(|left, right| {
        severity_name(left.severity)
            .cmp(severity_name(right.severity))
            .then_with(|| code_name(left.code).cmp(code_name(right.code)))
            .then_with(|| left.message.cmp(&right.message))
    });
    let summary = summarize(&findings);

    DoctorReport {
        store: store_root.to_path_buf(),
        findings,
        summary,
        deliveries,
        installation_plan,
    }
}

fn check_autosync(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) {
    match autosync::status(paths) {
        Ok(status) => {
            if let Some(error) = &status.scheduler_error {
                findings.push(finding_error(
                    DoctorCode::AutosyncStateInvalid,
                    format!("native autosync scheduler could not be inspected: {error}"),
                    Some("dalo autosync status".to_owned()),
                ));
            }
            if !status.installed && status.configured {
                findings.push(finding_warning(
                    DoctorCode::AutosyncDisabled,
                    "config enables autosync, but scheduler installation metadata is missing",
                    Some("dalo autosync install".to_owned()),
                ));
            } else if !status.installed {
                findings.push(info(
                    DoctorCode::AutosyncNotInstalled,
                    "scheduled synchronization is not installed",
                ));
            } else if let Some(executable) = status
                .executable
                .as_ref()
                .filter(|path| !autosync::executable_available(path))
            {
                findings.push(finding_warning(
                    DoctorCode::AutosyncExecutableMissing,
                    format!(
                        "recorded autosync executable `{}` is missing or not executable",
                        executable.display()
                    ),
                    Some("dalo autosync install".to_owned()),
                ));
            } else if status.enabled && status.configured {
                findings.push(ok(
                    DoctorCode::AutosyncInstalled,
                    format!(
                        "scheduled synchronization is enabled via {} ({})",
                        status.backend.map_or("unknown", |backend| backend.as_str()),
                        status
                            .schedule
                            .map_or("unknown", |schedule| schedule.as_str())
                    ),
                ));
            } else {
                findings.push(finding_warning(
                    DoctorCode::AutosyncDisabled,
                    status.disabled_reason.as_deref().map_or_else(
                        || {
                            "autosync config, metadata, and native scheduler state are inconsistent"
                                .to_owned()
                        },
                        |reason| format!("autosync is installed but disabled: {reason}"),
                    ),
                    Some("dalo autosync install".to_owned()),
                ));
            }
            // Run-state findings only apply to an installed job, matching the
            // `status --check` gate; a run recorded without an install must not
            // disagree with that command.
            if let Some(run) = &status.last_run
                && status.installed
            {
                if run.outcome == AutosyncRunOutcome::Blocked {
                    findings.push(finding_warning(
                        DoctorCode::AutosyncRunBlocked,
                        format!(
                            "latest scheduled synchronization was blocked: {}",
                            run.reason.as_deref().unwrap_or("no reason recorded")
                        ),
                        Some("dalo autosync status".to_owned()),
                    ));
                } else if autosync::running_run_is_stale(run, status.schedule, autosync::now_unix())
                {
                    findings.push(finding_warning(
                        DoctorCode::AutosyncRunStale,
                        "a scheduled synchronization started but never recorded a terminal outcome; it was likely interrupted",
                        Some("dalo autosync status".to_owned()),
                    ));
                }
            }
        }
        Err(error) => findings.push(finding_error(
            DoctorCode::AutosyncStateInvalid,
            format!("autosync state could not be inspected: {error}"),
            Some("dalo autosync uninstall".to_owned()),
        )),
    }
}

fn check_commands(findings: &mut Vec<DoctorFinding>) {
    if command_succeeds("git", &["--version"]) {
        findings.push(ok(DoctorCode::GitAvailable, "git is available"));
    } else {
        findings.push(finding_error(
            DoctorCode::GitMissing,
            "git is not available on PATH",
            None,
        ));
    }
}

fn check_store_layout(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) {
    if paths.root.is_dir() {
        findings.push(ok(DoctorCode::StoreExists, "store root exists"));
    } else {
        findings.push(finding_error(
            DoctorCode::StoreMissing,
            format!("store root `{}` does not exist", paths.root.display()),
            Some("dalo init".to_owned()),
        ));
        return;
    }

    for path in [
        &paths.config_file,
        &paths.lock_file,
        &paths.state_file,
        &paths.approvals_file,
        &paths.local_dir,
        &paths.local_skills_dir,
        &paths.sources_dir,
    ] {
        if path.exists() {
            findings.push(ok(
                DoctorCode::StoreLayoutOk,
                format!("expected store path exists: `{}`", path.display()),
            ));
        } else {
            findings.push(finding_error(
                DoctorCode::StoreLayoutMissing,
                format!("expected store path is missing: `{}`", path.display()),
                Some("dalo init".to_owned()),
            ));
        }
    }
}

fn read_config(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<UserConfig> {
    // A merely-missing file is already reported by `check_store_layout` as
    // `store_layout_missing` with a `dalo init` hint. Re-reporting it here as
    // `*_invalid` would duplicate that finding and, for config, attach a
    // dead-end `$EDITOR` hint pointing at a path that does not exist.
    if !paths.config_file.exists() {
        return None;
    }
    match store::read_config(paths) {
        Ok(config) => {
            findings.push(ok(DoctorCode::ConfigOk, "config parses"));
            Some(config)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::ConfigInvalid,
                format!("config could not be read: {error}"),
                Some(format!("$EDITOR {}", shell_quote_path(&paths.config_file))),
            ));
            None
        }
    }
}

fn read_state(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<StateFile> {
    // A missing file is already surfaced as `store_layout_missing`.
    if !paths.state_file.exists() {
        return None;
    }
    match store::read_state(paths) {
        Ok(state) => {
            findings.push(ok(DoctorCode::StateOk, "state parses"));
            Some(state)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::StateInvalid,
                format!("state could not be read: {error}"),
                Some("dalo init".to_owned()),
            ));
            None
        }
    }
}

fn read_lock(
    paths: &StorePaths,
    findings: &mut Vec<DoctorFinding>,
) -> Option<crate::lockfile::UserLock> {
    // A missing file is already surfaced as `store_layout_missing`.
    if !paths.lock_file.exists() {
        return None;
    }
    match store::read_user_lock(paths) {
        Ok(lock) => {
            findings.push(ok(DoctorCode::LockOk, "user lock parses"));
            Some(lock)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::LockInvalid,
                format!("user lock could not be read: {error}"),
                Some(format!(
                    "inspect {}; repair it or restore a known-good backup before running `dalo sync`; do not remove it because it records active instruction packs",
                    shell_quote_path(&paths.lock_file)
                )),
            ));
            None
        }
    }
}

fn read_approvals(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> Option<ApprovalsFile> {
    // A missing file is already surfaced as `store_layout_missing`; avoid the
    // dead-end "inspect or restore approvals.toml" hint for a path that is gone.
    if !paths.approvals_file.exists() {
        return None;
    }
    match store::read_approvals(paths) {
        Ok(approvals) => {
            findings.push(ok(DoctorCode::ApprovalsOk, "approvals parse"));
            Some(approvals)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::ApprovalsInvalid,
                format!("approvals could not be read: {error}"),
                Some(format!(
                    "$EDITOR {}",
                    shell_quote_path(&paths.approvals_file)
                )),
            ));
            None
        }
    }
}

enum SourceLockRead {
    Missing,
    Readable(SourceLock),
    Invalid,
}

impl SourceLockRead {
    fn lock(&self) -> Option<&SourceLock> {
        match self {
            Self::Readable(lock) => Some(lock),
            Self::Missing | Self::Invalid => None,
        }
    }

    fn can_check_provenance(&self) -> bool {
        !matches!(self, Self::Invalid)
    }
}

fn read_source_lock(paths: &StorePaths, findings: &mut Vec<DoctorFinding>) -> SourceLockRead {
    if !paths.source_lock_file.exists() {
        return SourceLockRead::Missing;
    }

    match catalog::read_source_lock(paths) {
        Ok(lock) => {
            findings.push(ok(
                DoctorCode::SourceLockOk,
                "catalog source lock is present and readable",
            ));
            SourceLockRead::Readable(lock)
        }
        Err(error) => {
            findings.push(finding_error(
                DoctorCode::SourceLockInvalid,
                format!("catalog source lock could not be read: {error}"),
                Some(format!(
                    "$EDITOR {}",
                    shell_quote_path(&paths.source_lock_file)
                )),
            ));
            SourceLockRead::Invalid
        }
    }
}

fn check_targets(state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for target in state.targets.iter().filter(|target| target.enabled) {
        if target.path.is_dir() {
            findings.push(ok(
                DoctorCode::TargetExists,
                format!(
                    "target `{}` exists at `{}`",
                    target.id,
                    target.path.display()
                ),
            ));
        } else {
            findings.push(finding_warning(
                DoctorCode::TargetMissing,
                format!(
                    "target `{}` is configured but `{}` is missing",
                    target.id,
                    target.path.display()
                ),
                Some(format!("dalo target link {}", target.id)),
            ));
        }

        if looks_cloud_synced(&target.path) {
            findings.push(finding_warning(
                DoctorCode::CloudSyncedTarget,
                format!(
                    "target `{}` appears to be inside a cloud-synced folder: `{}`",
                    target.id,
                    target.path.display()
                ),
                None,
            ));
        }
    }

    for dir in &state.materialization_dirs {
        if dir.logical_targets.len() > 1 {
            findings.push(info(
                DoctorCode::DuplicateTargetDirectory,
                format!(
                    "targets share `{}`: {}",
                    dir.path.display(),
                    dir.logical_targets.join(", ")
                ),
            ));
        }
    }
}

fn check_owned_symlinks(paths: &StorePaths, state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for owned in &state.owned_skills {
        match fs::symlink_metadata(&owned.link_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                match fs::read_link(&owned.link_path) {
                    Ok(target) => {
                        let resolved = store::resolve_link_target(&owned.link_path, &target);
                        if !store::path_is_same_or_descendant(&resolved, &paths.root) {
                            findings.push(finding_error(
                                DoctorCode::ForeignOwnedSymlink,
                                format!(
                                    "owned symlink `{}` points outside the store to `{}`",
                                    owned.link_path.display(),
                                    target.display()
                                ),
                                Some(format!(
                                    "dalo resolve remove-owned {}",
                                    owned_selector(owned)
                                )),
                            ));
                        } else if !resolved.exists() {
                            findings.push(finding_error(
                                DoctorCode::BrokenOwnedSymlink,
                                format!(
                                    "owned symlink `{}` points to missing `{}`",
                                    owned.link_path.display(),
                                    target.display()
                                ),
                                Some(format!(
                                    "dalo resolve remove-owned {}",
                                    owned_selector(owned)
                                )),
                            ));
                        } else if store::comparable_path(&resolved)
                            != store::comparable_path(&owned.store_path)
                        {
                            findings.push(finding_error(
                                DoctorCode::OwnedSymlinkRepointed,
                                format!(
                                    "owned symlink `{}` points to `{}`, but its recorded store path is `{}`",
                                    owned.link_path.display(),
                                    resolved.display(),
                                    owned.store_path.display()
                                ),
                                Some(format!(
                                    "dalo resolve remove-owned {}",
                                    owned_selector(owned)
                                )),
                            ));
                        } else {
                            findings.push(ok(
                                DoctorCode::OwnedSymlinkOk,
                                format!("owned symlink `{}` is valid", owned.link_path.display()),
                            ));
                        }
                    }
                    Err(error) => findings.push(finding_error(
                        DoctorCode::BrokenOwnedSymlink,
                        format!(
                            "owned symlink `{}` could not be read: {error}",
                            owned.link_path.display()
                        ),
                        Some(format!(
                            "dalo resolve remove-owned {}",
                            owned_selector(owned)
                        )),
                    )),
                }
            }
            Ok(_) => findings.push(finding_error(
                DoctorCode::OwnedPathRealEntry,
                format!(
                    "recorded owned path `{}` is a real entry, not a symlink",
                    owned.link_path.display()
                ),
                Some(format!(
                    "dalo resolve remove-owned {}",
                    owned_selector(owned)
                )),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                findings.push(finding_warning(
                    DoctorCode::MissingOwnedSymlink,
                    format!(
                        "recorded owned symlink `{}` is missing",
                        owned.link_path.display()
                    ),
                    Some(format!(
                        "dalo resolve remove-owned {}",
                        owned_selector(owned)
                    )),
                ));
            }
            Err(error) => findings.push(finding_error(
                DoctorCode::BrokenOwnedSymlink,
                format!(
                    "recorded owned symlink `{}` could not be inspected: {error}",
                    owned.link_path.display()
                ),
                Some(format!(
                    "dalo resolve remove-owned {}",
                    owned_selector(owned)
                )),
            )),
        }
    }
}

fn check_protected_skills(state: &StateFile, findings: &mut Vec<DoctorFinding>) {
    for protected in &state.protected_skills {
        let target = state
            .targets
            .iter()
            .find(|target| target.id == protected.target_id && target.enabled);
        let path = target
            .map(|target| target.canonical_path.join(&protected.slot_name))
            .or_else(|| protected.path.clone());
        let selector = if protected.target_id.is_empty() {
            protected.slot_name.clone()
        } else {
            format!("{}:{}", protected.target_id, protected.slot_name)
        };
        if target.is_none() || path.as_ref().is_none_or(|path| !path.is_dir()) {
            findings.push(finding_warning(
                DoctorCode::StaleProtectedSkill,
                format!(
                    "protected slot `{selector}` no longer maps to an existing target directory{}",
                    path.as_ref()
                        .map_or_else(String::new, |path| format!(" at `{}`", path.display()))
                ),
                Some(format!("dalo resolve unkeep {selector}")),
            ));
        } else {
            findings.push(info(
                DoctorCode::ProtectedSkillKept,
                format!(
                    "protected slot `{selector}` is kept at `{}`",
                    path.expect("existing protected path should be present")
                        .display()
                ),
            ));
        }
    }
}

fn owned_selector(owned: &OwnedSkillState) -> String {
    format!("{}:{}", owned.target_id, owned.slot_name)
}

fn check_sources(
    config: &UserConfig,
    source_lock: &SourceLockRead,
    findings: &mut Vec<DoctorFinding>,
) {
    for source in config.sources.iter().filter(|source| source.enabled) {
        // An enabled source whose checkout cannot be read degrades sync, so it
        // must be a hard error (matching `status`/`sync --check`), not a vague
        // "could not check dirty state" warning that lets `doctor --check`
        // pass. `try_exists` distinguishes a confirmed-absent path from a stat
        // error (e.g. permission denied on a parent) so the message is accurate.
        match source.path.try_exists() {
            Ok(false) => {
                findings.push(finding_error(
                    DoctorCode::SourceMissing,
                    format!(
                        "source `{}` checkout is missing at `{}`; restore it or run `dalo source remove {}`",
                        source.id,
                        source.path.display(),
                        source.id
                    ),
                    None,
                ));
                continue;
            }
            Err(error) => {
                findings.push(finding_error(
                    DoctorCode::SourceMissing,
                    format!(
                        "source `{}` checkout at `{}` could not be read: {error}; restore it or run `dalo source remove {}`",
                        source.id,
                        source.path.display(),
                        source.id
                    ),
                    None,
                ));
                continue;
            }
            Ok(true) => {}
        }
        match git::is_dirty(&source.path) {
            Ok(true) => {
                let severity = if source.kind == SourceKind::Team {
                    DoctorSeverity::Error
                } else {
                    DoctorSeverity::Warning
                };
                findings.push(DoctorFinding {
                    severity,
                    code: DoctorCode::DirtySource,
                    message: if source.kind == SourceKind::Local {
                        format!(
                            "source `{}` at `{}` has local changes; adopted skills must be committed before syncing",
                            source.id,
                            source.path.display()
                        )
                    } else {
                        format!(
                            "source `{}` at `{}` has local changes; resolve or commit them before syncing",
                            source.id,
                            source.path.display()
                        )
                    },
                    next_command: Some(format!(
                        "git -C {} status",
                        shell_quote_path(&source.path)
                    )),
                    inventory_warnings: Vec::new(),
                });
            }
            Ok(false) => findings.push(ok(
                DoctorCode::SourceClean,
                format!("source `{}` is clean", source.id),
            )),
            Err(error) => findings.push(finding_warning(
                DoctorCode::DirtySource,
                format!(
                    "source `{}` dirty state could not be checked: {error}",
                    source.id
                ),
                None,
            )),
        }
        if source.declared_by.is_some() && source_lock.can_check_provenance() {
            check_manifest_source_provenance(source, config, source_lock.lock(), findings);
        }
    }
}

fn check_source_inventories(
    live_resolution: Option<&resolver::LiveResolution>,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(live_resolution) = live_resolution else {
        return;
    };
    for scan in &live_resolution.scans {
        match (&scan.inventory, &scan.error) {
            (Some(inventory), _) if resolver::inventory_degrades_source_for_removal(inventory) => {
            let details = inventory
                .warnings
                .iter()
                .map(|warning| {
                    format!(
                        "{} at `{}`: {}",
                        warning.code,
                        warning.path.display(),
                        warning.message
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            findings.push(DoctorFinding {
                severity: DoctorSeverity::Error,
                code: DoctorCode::SourceInventoryDegraded,
                message: format!(
                    "source `{}` inventory is degraded; sync preserves existing links: {details}",
                    scan.source.id
                ),
                next_command: Some(source_inventory_fix_hint(
                    &scan.source,
                    &inventory.warnings,
                )),
                inventory_warnings: inventory.warnings.clone(),
            });
            }
            // `check_sources` already reports a missing or unreadable checkout.
            // Only add a distinct inventory error when the checkout itself is
            // present but scanning it failed.
            (None, Some(error)) if scan.source.path.try_exists().is_ok_and(|exists| exists) => findings.push(finding_error(
                DoctorCode::SourceInventoryDegraded,
                format!(
                    "source `{}` inventory could not be scanned: {error}; sync preserves existing links",
                    scan.source.id
                ),
                Some("dalo status".to_owned()),
            )),
            _ => {}
        }
    }
}

fn source_inventory_fix_hint(source: &SourceConfig, warnings: &[InventoryWarning]) -> String {
    if let Some(warning) = warnings
        .iter()
        .find(|warning| warning.code == InventoryWarningCode::UnreadablePath)
    {
        let access_recovery = format!(
            "restore read access to {} in Dalo's managed checkout",
            shell_quote_path(&warning.path)
        );
        if let Some(invalid) = warnings
            .iter()
            .find(|warning| warning.code == InventoryWarningCode::InvalidSlotName)
        {
            if source.kind == SourceKind::Local {
                let repair = if invalid.message.starts_with("frontmatter name ") {
                    format!(
                        "change the frontmatter `name` in {} to a portable lowercase slot name",
                        shell_quote_path(&invalid.path)
                    )
                } else {
                    format!(
                        "rename {} to a portable lowercase slot name",
                        shell_quote_path(invalid.path.parent().unwrap_or(&invalid.path))
                    )
                };
                return format!("{access_recovery}; then {repair}, then run `dalo sync`");
            }
            return format!(
                "{access_recovery}; then fix the invalid skill name for source `{}` in its upstream repository, push it, then run `dalo sync`",
                source.id
            );
        }
        return format!("{access_recovery}, then run `dalo sync`");
    }
    if let Some(warning) = warnings
        .iter()
        .find(|warning| warning.code == InventoryWarningCode::InvalidSlotName)
    {
        if warning.message.starts_with("frontmatter name ") {
            if source.kind != SourceKind::Local {
                return format!(
                    "fix the frontmatter `name` for source `{}` in its upstream repository, push it, then run `dalo sync`",
                    source.id
                );
            }
            return format!(
                "change the frontmatter `name` in {} to a portable lowercase slot name, then run `dalo sync`",
                shell_quote_path(&warning.path)
            );
        }
        if source.kind != SourceKind::Local {
            return format!(
                "rename the affected skill folder for source `{}` in its upstream repository, push it, then run `dalo sync`",
                source.id
            );
        }
        let path = warning.path.parent().unwrap_or(&warning.path);
        return format!(
            "rename {} to a portable lowercase slot name, then run `dalo sync`",
            shell_quote_path(path)
        );
    }
    if let Some(warning) = warnings
        .iter()
        .find(|warning| warning.code == InventoryWarningCode::SkippedSymlink)
    {
        if source.kind != SourceKind::Local {
            return format!(
                "replace the unsafe symlink for source `{}` in its upstream repository, push it, then run `dalo sync`",
                source.id
            );
        }
        return format!(
            "replace the unsafe symlink at {} with a real path inside the source, then run `dalo sync`",
            shell_quote_path(&warning.path)
        );
    }
    if source.kind != SourceKind::Local {
        return format!(
            "fix the source inventory warning for source `{}` in its upstream repository, push it, then run `dalo sync`",
            source.id
        );
    }
    "fix the source inventory warning, then run `dalo sync`".to_owned()
}

fn check_manifest_source_provenance(
    source: &SourceConfig,
    config: &UserConfig,
    source_lock: Option<&SourceLock>,
    findings: &mut Vec<DoctorFinding>,
) {
    let Some(team_id) = source.declared_by.as_deref() else {
        return;
    };
    let mut mismatches = Vec::new();
    let lock_commit = source_lock
        .and_then(|lock| lock.catalog(&source.id))
        .map(|entry| entry.commit.as_str());
    let checkout_commit = git::rev_parse_head(&source.path).ok();
    match (lock_commit, checkout_commit.as_deref()) {
        (None, _) => mismatches.push("source-lock.toml has no catalog pin".to_owned()),
        (Some(pin), Some(checkout)) if pin != checkout => mismatches.push(format!(
            "checkout {} does not match source-lock pin {}",
            short_commit(checkout),
            short_commit(pin)
        )),
        (Some(_), None) => mismatches.push("checkout commit could not be read".to_owned()),
        _ => {}
    }

    let team = config
        .sources
        .iter()
        .find(|candidate| candidate.id == team_id);
    if let Some(team) = team {
        match crate::team_manifest::load_team_manifest(&team.path, team_id) {
            Ok(manifest) => {
                let declaration = manifest.catalogs.iter().find(|catalog| {
                    crate::team_manifest::source_matches_owned_declaration(source, &catalog.id)
                });
                if let Some(declaration) = declaration {
                    let expected_url =
                        source::resolve_source_location(&declaration.url, &team.path);
                    if source.url.as_deref() != Some(expected_url.as_str()) {
                        mismatches
                            .push("manifest origin does not match configured origin".to_owned());
                    }
                    if source.declared_ref.as_deref() != Some(declaration.version.as_str()) {
                        mismatches
                            .push("manifest version does not match configured version".to_owned());
                    }
                } else {
                    mismatches.push("declaring team manifest has no matching catalog".to_owned());
                }
            }
            Err(error) => mismatches.push(format!("declaring team manifest is invalid: {error}")),
        }
    } else {
        mismatches.push(format!("declaring team source `{team_id}` is missing"));
    }

    if mismatches.is_empty() {
        let provenance = source::source_provenance(source, source_lock);
        let origin = provenance.origin_url.as_deref().unwrap_or("<unknown>");
        let requested = provenance.requested_ref.as_deref().unwrap_or("<unknown>");
        let resolved = provenance
            .resolved_commit
            .as_deref()
            .map(short_commit)
            .unwrap_or("<missing>");
        findings.push(ok(
            DoctorCode::SourceProvenanceOk,
            format!(
                "manifest-derived source `{}` from `{origin}` requested `{requested}` and resolved `{resolved}`",
                source.id
            ),
        ));
    } else {
        findings.push(finding_error(
            DoctorCode::SourceProvenanceMismatch,
            format!(
                "manifest-derived source `{}` has provenance mismatch: {}",
                source.id,
                mismatches.join("; ")
            ),
            Some("dalo sync".to_owned()),
        ));
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
}

fn check_source_store_debris(
    paths: &StorePaths,
    config: &UserConfig,
    findings: &mut Vec<DoctorFinding>,
) {
    let configured = config
        .sources
        .iter()
        .filter(|source| source.kind != SourceKind::Local)
        .map(|source| source.id.as_str())
        .collect::<BTreeSet<_>>();
    let Ok(source_dirs) = fs::read_dir(&paths.sources_dir) else {
        return;
    };

    for entry in source_dirs.flatten() {
        let source_id = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        // dalo-internal staging areas hold detached worktrees created during a
        // sync or audit. They are expected to be empty between operations, so an
        // empty directory is not debris; any leftover per-`{source}-{commit}`
        // subtree is interrupted-operation debris, not removable "unconfigured
        // source content".
        if source_id == ".manifest-staging" || source_id == ".audit-staging" {
            let Ok(children) = fs::read_dir(&path) else {
                continue;
            };
            for child in children.flatten() {
                findings.push(finding_warning(
                    DoctorCode::SourceStoreDebris,
                    format!(
                        "interrupted source-operation debris exists at `{}`; inspect it and remove it if it is unwanted",
                        child.path().display()
                    ),
                    None,
                ));
            }
            continue;
        }
        if !configured.contains(source_id.as_str()) {
            findings.push(finding_warning(
                DoctorCode::SourceStoreDebris,
                format!(
                    "unconfigured source content exists at `{}`; inspect it and remove it if it is not managed by dalo",
                    path.display()
                ),
                None,
            ));
            continue;
        }

        let Ok(children) = fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let name = child.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".checkout-tmp-") || name == "checkout.dalo-removing" {
                findings.push(finding_warning(
                    DoctorCode::SourceStoreDebris,
                    format!(
                        "interrupted source-operation debris exists at `{}`; inspect it and remove it if it is unwanted",
                        child.path().display()
                    ),
                    None,
                ));
            }
        }
    }
}

fn check_resolution(
    paths: &StorePaths,
    config: &UserConfig,
    live_resolution: &resolver::LiveResolution,
    approvals_present: bool,
    findings: &mut Vec<DoctorFinding>,
) {
    let resolution = &live_resolution.resolution;

    if approvals_present {
        for diagnostic in &resolution.diagnostics {
            if diagnostic.code == resolver::ResolutionDiagnosticCode::LegacyBareApproval {
                findings.push(finding_warning(
                    DoctorCode::PendingApproval,
                    diagnostic.message.clone(),
                    Some("dalo status".to_owned()),
                ));
            }
        }
        for skill in &resolution.pending_approval_skills {
            findings.push(finding_warning(
                DoctorCode::PendingApproval,
                format!("skill `{}` is pending approval", skill.source_ref),
                Some(format!("dalo approve skill {}", skill.source_ref)),
            ));
        }
    }

    for blocked in &resolution.blocked_skills {
        findings.push(finding_warning(
            DoctorCode::RequiredClosureBlocked,
            format!(
                "skill `{}` is blocked: requirement `{}` is {}",
                blocked.skill.source_ref,
                blocked.requirement,
                resolver::closure_block_reason_name(blocked.reason)
            ),
            Some("dalo status".to_owned()),
        ));
    }

    // Mirror the deterministic security gate that `status`/`sync --check` apply
    // so `doctor --check` does not report a healthy store while sync would
    // refuse to link a skill with an unaccepted blocking finding. Read-only
    // (persist = false), so it never writes audit reports.
    let audits = audit::audit_active_skills_with_config(paths, resolution, false, config);
    for source_ref in &audits.blocking {
        findings.push(finding_error(
            DoctorCode::SecurityAuditBlocked,
            format!(
                "active skill `{source_ref}` is blocked by an unaccepted security-audit finding"
            ),
            Some(format!("dalo audit {source_ref}")),
        ));
    }
    for failure in &audits.failures {
        findings.push(finding_warning(
            DoctorCode::SecurityAuditFailed,
            format!(
                "security audit for active skill `{}` could not be completed: {}",
                failure.source_ref, failure.reason
            ),
            Some(format!("dalo audit {}", failure.source_ref)),
        ));
    }

    let lock = store::read_user_lock(paths).unwrap_or_default();
    let discovered =
        instructions::discover_packs(paths, &config.sources, &lock.active_instruction_packs);
    let active_packs = discovered
        .into_iter()
        .filter(|pack| pack.enabled)
        .collect::<Vec<_>>();
    for overlap in instructions::topic_overlaps(&active_packs) {
        findings.push(finding_warning(
            DoctorCode::InstructionPackTopicOverlap,
            format!(
                "instruction packs `{}` and `{}` overlap on topics: {}",
                overlap.packs[0],
                overlap.packs[1],
                overlap.topics.join(", ")
            ),
            Some("dalo status".to_owned()),
        ));
    }
    for drift in instructions::instruction_block_drifts(
        paths,
        &config.sources,
        &lock.active_instruction_packs,
    ) {
        findings.push(finding_warning(
            DoctorCode::InstructionBlockDrift,
            format!(
                "instruction pack `{}:{}` is {} at `{}`: {}",
                drift.source_id,
                drift.pack_id,
                instruction_block_drift_kind_name(drift.kind),
                drift.target.display(),
                drift.message
            ),
            Some(format!(
                "dalo instructions enable {} {}",
                drift.pack_id,
                drift.target.display()
            )),
        ));
    }

    let active_slots = resolution
        .active_skills
        .iter()
        .map(|skill| (skill.slot_name.as_str(), skill.source_ref.as_str()))
        .collect::<BTreeMap<_, _>>();
    if let Ok(unmanaged_scan) = adopt::discover_unmanaged_skill_scan(paths) {
        for warning in unmanaged_scan.warnings {
            findings.push(finding_warning(
                DoctorCode::UnreadableTargetDirectory,
                format!(
                    "target path `{}` could not be scanned: {}",
                    warning.path.display(),
                    warning.message
                ),
                None,
            ));
        }
        for unmanaged in unmanaged_scan.unmanaged_skills {
            if unmanaged.protected {
                continue;
            }
            if let Some(source_ref) = active_slots.get(unmanaged.slot_name.as_str()) {
                findings.push(finding_error(
                    DoctorCode::UnmanagedSameNameBlocker,
                    format!(
                        "unmanaged skill `{}` blocks managed `{}`",
                        unmanaged.path.display(),
                        source_ref
                    ),
                    Some(format!("dalo adopt {}", unmanaged.id)),
                ));
            }
        }
    }
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    command_succeeds_with_timeout(program, args, COMMAND_CHECK_TIMEOUT)
}

fn command_succeeds_with_timeout(program: &str, args: &[&str], timeout: Duration) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {}
            Err(_) => return false,
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }

        thread::sleep(COMMAND_CHECK_POLL_INTERVAL.min(timeout - elapsed));
    }
}

fn looks_cloud_synced(path: &Path) -> bool {
    let value = path.to_string_lossy();
    ["Dropbox", "Google Drive", "iCloud Drive", "OneDrive"]
        .iter()
        .any(|marker| value.contains(marker))
}

fn summarize(findings: &[DoctorFinding]) -> DoctorSummary {
    let mut summary = DoctorSummary::default();
    for finding in findings {
        match finding.severity {
            DoctorSeverity::Error => summary.errors += 1,
            DoctorSeverity::Warning => summary.warnings += 1,
            DoctorSeverity::Info => summary.info += 1,
            DoctorSeverity::Ok => summary.ok += 1,
        }
    }
    summary
}

fn ok(code: DoctorCode, message: impl Into<String>) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Ok,
        code,
        message: message.into(),
        next_command: None,
        inventory_warnings: Vec::new(),
    }
}

fn info(code: DoctorCode, message: impl Into<String>) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Info,
        code,
        message: message.into(),
        next_command: None,
        inventory_warnings: Vec::new(),
    }
}

fn finding_warning(
    code: DoctorCode,
    message: impl Into<String>,
    next_command: Option<String>,
) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code,
        message: message.into(),
        next_command,
        inventory_warnings: Vec::new(),
    }
}

fn finding_error(
    code: DoctorCode,
    message: impl Into<String>,
    next_command: Option<String>,
) -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Error,
        code,
        message: message.into(),
        next_command,
        inventory_warnings: Vec::new(),
    }
}

fn severity_name(severity: DoctorSeverity) -> &'static str {
    match severity {
        DoctorSeverity::Error => "0_error",
        DoctorSeverity::Warning => "1_warning",
        DoctorSeverity::Info => "2_info",
        DoctorSeverity::Ok => "3_ok",
    }
}

fn code_name(code: DoctorCode) -> &'static str {
    match code {
        DoctorCode::StoreExists => "store_exists",
        DoctorCode::StoreMissing => "store_missing",
        DoctorCode::StoreLayoutOk => "store_layout_ok",
        DoctorCode::StoreLayoutMissing => "store_layout_missing",
        DoctorCode::ConfigOk => "config_ok",
        DoctorCode::ConfigInvalid => "config_invalid",
        DoctorCode::StateOk => "state_ok",
        DoctorCode::StateInvalid => "state_invalid",
        DoctorCode::LockOk => "lock_ok",
        DoctorCode::LockInvalid => "lock_invalid",
        DoctorCode::SourceLockOk => "source_lock_ok",
        DoctorCode::SourceLockInvalid => "source_lock_invalid",
        DoctorCode::ApprovalsOk => "approvals_ok",
        DoctorCode::ApprovalsInvalid => "approvals_invalid",
        DoctorCode::GitAvailable => "git_available",
        DoctorCode::GitMissing => "git_missing",
        DoctorCode::LocalGitOk => "local_git_ok",
        DoctorCode::LocalGitMissing => "local_git_missing",
        DoctorCode::TargetExists => "target_exists",
        DoctorCode::TargetMissing => "target_missing",
        DoctorCode::DuplicateTargetDirectory => "duplicate_target_directory",
        DoctorCode::OwnedSymlinkOk => "owned_symlink_ok",
        DoctorCode::MissingOwnedSymlink => "missing_owned_symlink",
        DoctorCode::BrokenOwnedSymlink => "broken_owned_symlink",
        DoctorCode::OwnedPathRealEntry => "owned_path_real_entry",
        DoctorCode::ForeignOwnedSymlink => "foreign_owned_symlink",
        DoctorCode::OwnedSymlinkRepointed => "owned_symlink_repointed",
        DoctorCode::UnmanagedSameNameBlocker => "unmanaged_same_name_blocker",
        DoctorCode::ProtectedSkillKept => "protected_skill_kept",
        DoctorCode::StaleProtectedSkill => "stale_protected_skill",
        DoctorCode::UnreadableTargetDirectory => "unreadable_target_directory",
        DoctorCode::SourceClean => "source_clean",
        DoctorCode::DirtySource => "dirty_source",
        DoctorCode::SourceMissing => "source_missing",
        DoctorCode::SourceInventoryDegraded => "source_inventory_degraded",
        DoctorCode::SourceProvenanceOk => "source_provenance_ok",
        DoctorCode::SourceProvenanceMismatch => "source_provenance_mismatch",
        DoctorCode::SourceStoreDebris => "source_store_debris",
        DoctorCode::PendingApproval => "pending_approval",
        DoctorCode::RequiredClosureBlocked => "required_closure_blocked",
        DoctorCode::SecurityAuditBlocked => "security_audit_blocked",
        DoctorCode::SecurityAuditFailed => "security_audit_failed",
        DoctorCode::ToolPendingApproval => "tool_pending_approval",
        DoctorCode::ToolHashDrift => "tool_hash_drift",
        DoctorCode::ToolRuntimeMissing => "tool_runtime_missing",
        DoctorCode::ToolPlatformMismatch => "tool_platform_mismatch",
        DoctorCode::ToolApprovalRevoked => "tool_approval_revoked",
        DoctorCode::ToolAuditFailed => "tool_audit_failed",
        DoctorCode::ToolReady => "tool_ready",
        DoctorCode::ToolStagingDebris => "tool_staging_debris",
        DoctorCode::GeneratedDeliveryInvalid => "generated_delivery_invalid",
        DoctorCode::GeneratedDeliveryStagingDebris => "generated_delivery_staging_debris",
        DoctorCode::HookPendingApproval => "hook_pending_approval",
        DoctorCode::HookHashDrift => "hook_hash_drift",
        DoctorCode::HookToolUnavailable => "hook_tool_unavailable",
        DoctorCode::HookReady => "hook_ready",
        DoctorCode::HookProviderDisabled => "hook_provider_disabled",
        DoctorCode::HookProviderUnverified => "hook_provider_unverified",
        DoctorCode::HookNativeConflict => "hook_native_conflict",
        DoctorCode::PluginProjectionBlocked => "plugin_projection_blocked",
        DoctorCode::PluginProjectionConflict => "plugin_projection_conflict",
        DoctorCode::InstructionPackTopicOverlap => "instruction_pack_topic_overlap",
        DoctorCode::InstructionBlockDrift => "instruction_block_drift",
        DoctorCode::CloudSyncedTarget => "cloud_synced_target",
        DoctorCode::AutosyncInstalled => "autosync_installed",
        DoctorCode::AutosyncNotInstalled => "autosync_not_installed",
        DoctorCode::AutosyncDisabled => "autosync_disabled",
        DoctorCode::AutosyncExecutableMissing => "autosync_executable_missing",
        DoctorCode::AutosyncRunBlocked => "autosync_run_blocked",
        DoctorCode::AutosyncRunStale => "autosync_run_stale",
        DoctorCode::AutosyncStateInvalid => "autosync_state_invalid",
    }
}

fn instruction_block_drift_kind_name(
    kind: instructions::InstructionBlockDriftKind,
) -> &'static str {
    match kind {
        instructions::InstructionBlockDriftKind::Missing => "missing",
        instructions::InstructionBlockDriftKind::Malformed => "malformed",
        instructions::InstructionBlockDriftKind::Stale => "stale",
        instructions::InstructionBlockDriftKind::SourceMissing => "source-missing",
    }
}

impl std::fmt::Display for DoctorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(code_name(*self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MaterializationDirState, OwnedSkillState, TargetState};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn run_doctor_should_report_missing_store_without_creating_it() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("missing-store");

        let report = run_doctor(&store);

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].code, DoctorCode::StoreMissing);
        assert_eq!(report.findings[0].severity, DoctorSeverity::Error);
        assert_eq!(
            report.findings[0].next_command.as_deref(),
            Some(store::dalo_command(&store, "init").as_str())
        );
        assert_eq!(
            report.summary,
            DoctorSummary {
                errors: 1,
                warnings: 0,
                info: 0,
                ok: 0,
            }
        );
        assert!(!store.exists());
    }

    #[test]
    fn run_doctor_should_not_report_unimplemented_github_pr_readiness() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");

        let report = run_doctor(&store);
        let serialized = serde_json::to_string(&report).expect("doctor report should serialize");

        assert!(
            !serialized.contains("gh_"),
            "doctor must not report GitHub CLI readiness before dalo has a PR flow: {serialized}"
        );
    }

    #[test]
    fn run_doctor_should_report_inventory_degradation_as_an_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let invalid_skill = store.join("local/skills/Review");
        fs::create_dir_all(&invalid_skill).expect("invalid skill directory should be created");
        fs::write(invalid_skill.join("SKILL.md"), "# Review\n")
            .expect("invalid skill should be written");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::SourceInventoryDegraded)
            .expect("degraded inventory should be reported");
        assert_eq!(finding.severity, DoctorSeverity::Error);
        assert!(finding.message.contains("invalid_slot_name"));
        assert!(
            finding
                .message
                .contains(invalid_skill.to_string_lossy().as_ref())
        );
        assert!(
            finding
                .next_command
                .as_deref()
                .is_some_and(|hint| hint.contains("rename"))
        );
    }

    #[test]
    fn run_doctor_should_point_invalid_frontmatter_name_to_skill_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let skill = store.join("local/skills/review/SKILL.md");
        fs::create_dir_all(skill.parent().expect("skill should have a parent"))
            .expect("skill directory should be created");
        fs::write(&skill, "---\nname: Review\n---\n# Review\n").expect("skill should be written");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::SourceInventoryDegraded)
            .expect("degraded inventory should be reported");
        let hint = finding
            .next_command
            .as_deref()
            .expect("invalid frontmatter should include a repair hint");
        assert!(hint.contains("frontmatter `name`"));
        assert!(hint.contains(skill.to_string_lossy().as_ref()));
        assert!(!hint.contains("rename"));
    }

    #[test]
    fn mixed_inventory_hint_repairs_managed_checkout_access_before_upstream_content() {
        let source = SourceConfig {
            id: "team".to_owned(),
            kind: SourceKind::Team,
            path: PathBuf::from("/store/sources/team/checkout"),
            priority: 10,
            namespace: None,
            enabled: true,
            trusted: true,
            url: Some("https://example.com/team.git".to_owned()),
            branch: None,
            update_policy: Some("track".to_owned()),
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        };
        let hint = source_inventory_fix_hint(
            &source,
            &[
                InventoryWarning {
                    code: InventoryWarningCode::InvalidSlotName,
                    path: PathBuf::from("/store/sources/team/checkout/skills/bad/SKILL.md"),
                    message: "frontmatter name `bad name` is not portable".to_owned(),
                },
                InventoryWarning {
                    code: InventoryWarningCode::UnreadablePath,
                    path: PathBuf::from("/store/sources/team/checkout/skills/restricted"),
                    message: "permission denied".to_owned(),
                },
            ],
        );
        assert!(hint.starts_with(
            "restore read access to '/store/sources/team/checkout/skills/restricted' in Dalo's managed checkout"
        ));
        assert!(hint.contains(
            "then fix the invalid skill name for source `team` in its upstream repository"
        ));
        assert!(hint.ends_with("then run `dalo sync`"));

        let mut local = source;
        local.kind = SourceKind::Local;
        let hint = source_inventory_fix_hint(
            &local,
            &[
                InventoryWarning {
                    code: InventoryWarningCode::InvalidSlotName,
                    path: PathBuf::from("/store/local/skills/bad/SKILL.md"),
                    message: "frontmatter name `bad name` is not portable".to_owned(),
                },
                InventoryWarning {
                    code: InventoryWarningCode::UnreadablePath,
                    path: PathBuf::from("/store/local/skills/restricted"),
                    message: "permission denied".to_owned(),
                },
            ],
        );
        assert!(hint.contains("restore read access to '/store/local/skills/restricted'"));
        assert!(
            hint.contains("change the frontmatter `name` in '/store/local/skills/bad/SKILL.md'")
        );
    }

    #[test]
    fn read_source_lock_should_not_report_a_missing_file_as_readable() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let paths = StorePaths::new(temp_dir.path().join("store"));
        let mut findings = Vec::new();

        let source_lock = read_source_lock(&paths, &mut findings);

        assert!(matches!(source_lock, SourceLockRead::Missing));
        assert!(!findings.iter().any(|finding| {
            matches!(
                finding.code,
                DoctorCode::SourceLockOk | DoctorCode::SourceLockInvalid
            )
        }));
    }

    #[test]
    fn doctor_should_point_invalid_autosync_state_to_recovery_uninstall() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        fs::write(store.join("autosync.toml"), "not = [valid toml")
            .expect("autosync state should be corrupted");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::AutosyncStateInvalid)
            .expect("invalid autosync state should be reported");

        assert_eq!(
            finding.next_command.as_deref(),
            Some(store::dalo_command(&store, "autosync uninstall").as_str())
        );
    }

    #[test]
    fn doctor_should_name_the_missing_recorded_autosync_executable() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(store);
        let missing_executable = temp_dir.path().join("removed/dalo");
        let state = crate::autosync::AutosyncInstallState {
            schema_version: 1,
            backend: crate::autosync::SchedulerBackend::Cron,
            schedule: crate::autosync::AutosyncSchedule::Daily,
            executable: missing_executable.clone(),
            store: paths.root.clone(),
            identifier: "dalo-autosync-test".to_owned(),
            artifacts: vec!["crontab".to_owned()],
            installed_at_unix: 1,
        };
        fs::write(
            &paths.autosync_file,
            toml::to_string(&state).expect("autosync state should serialize"),
        )
        .expect("autosync state should be written");
        let mut config = store::read_config(&paths).expect("config should parse");
        config.settings.autosync = true;
        config.settings.sync_interval = Some("daily".to_owned());
        store::write_config(&paths, &config).expect("config should be written");
        let mut findings = Vec::new();

        check_autosync(&paths, &mut findings);

        let finding = findings
            .iter()
            .find(|finding| finding.code == DoctorCode::AutosyncExecutableMissing)
            .expect("missing executable should have a dedicated finding");
        assert_eq!(finding.severity, DoctorSeverity::Warning);
        assert!(
            finding
                .message
                .contains(&missing_executable.display().to_string())
        );
        assert_eq!(
            finding.next_command.as_deref(),
            Some("dalo autosync install")
        );
        assert!(
            !findings
                .iter()
                .any(|finding| finding.code == DoctorCode::AutosyncDisabled)
        );
    }

    #[test]
    fn doctor_should_flag_a_stale_running_autosync_run() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(store);

        let executable = temp_dir.path().join("bin/dalo");
        fs::create_dir_all(executable.parent().expect("binary has parent"))
            .expect("binary dir should exist");
        fs::write(&executable, "binary").expect("binary should exist");
        let mut permissions = fs::metadata(&executable)
            .expect("binary metadata readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("binary should be executable");
        let state = crate::autosync::AutosyncInstallState {
            schema_version: 1,
            backend: crate::autosync::SchedulerBackend::Cron,
            schedule: crate::autosync::AutosyncSchedule::Daily,
            executable,
            store: paths.root.clone(),
            identifier: "dalo-autosync-test".to_owned(),
            artifacts: vec!["crontab".to_owned()],
            installed_at_unix: 1,
        };
        fs::write(
            &paths.autosync_file,
            toml::to_string(&state).expect("autosync state should serialize"),
        )
        .expect("autosync state should be written");
        let mut config = store::read_config(&paths).expect("config should parse");
        config.settings.autosync = true;
        config.settings.sync_interval = Some("daily".to_owned());
        store::write_config(&paths, &config).expect("config should be written");

        // A run that started long ago but never recorded a terminal outcome.
        let run = crate::autosync::AutosyncRunState {
            schema_version: 1,
            last_attempted_at_unix: 1_000_000_000,
            last_successful_at_unix: None,
            outcome: crate::autosync::AutosyncRunOutcome::Running,
            reason: None,
        };
        fs::write(
            &paths.autosync_run_file,
            toml::to_string(&run).expect("run state should serialize"),
        )
        .expect("run state should be written");

        let mut findings = Vec::new();
        check_autosync(&paths, &mut findings);

        assert!(
            findings
                .iter()
                .any(|finding| finding.code == DoctorCode::AutosyncRunStale
                    && finding.severity == DoctorSeverity::Warning),
            "a stale running run should warn: {findings:?}"
        );
    }

    #[test]
    fn doctor_should_ignore_blocked_autosync_run_when_not_installed() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(store);

        // A blocked run persists, but no install metadata exists. `doctor` must
        // stay consistent with `status --check`, which ignores this case.
        let run = crate::autosync::AutosyncRunState {
            schema_version: 1,
            last_attempted_at_unix: 1_000_000_000,
            last_successful_at_unix: None,
            outcome: crate::autosync::AutosyncRunOutcome::Blocked,
            reason: Some("review required".to_owned()),
        };
        fs::write(
            &paths.autosync_run_file,
            toml::to_string(&run).expect("run state should serialize"),
        )
        .expect("run state should be written");

        let mut findings = Vec::new();
        check_autosync(&paths, &mut findings);

        assert!(
            !findings
                .iter()
                .any(|finding| finding.code == DoctorCode::AutosyncRunBlocked),
            "a run recorded without an install must not warn: {findings:?}"
        );
    }

    #[test]
    fn run_doctor_should_not_compare_provenance_when_source_lock_is_invalid() {
        use crate::config::Settings;

        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let catalog_repo = temp_dir.path().join("catalog");
        store::init_store(store.clone(), false).expect("init should succeed");
        create_git_skill_repo(&catalog_repo);
        let paths = StorePaths::new(store.clone());
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "team.marketing".to_owned(),
                kind: SourceKind::Catalog,
                path: catalog_repo,
                priority: 11,
                namespace: None,
                enabled: true,
                trusted: false,
                url: Some("https://example.com/marketing.git".to_owned()),
                branch: None,
                update_policy: Some("manifest".to_owned()),
                selection: Vec::new(),
                declared_by: Some("team".to_owned()),
                declared_ref: Some("main".to_owned()),
            }],
            plugins: crate::config::PluginConfig::default(),
            plugin_policy: Vec::new(),
        };
        store::write_config(&paths, &config).expect("config should be written");
        fs::write(&paths.source_lock_file, "schema_version = ")
            .expect("source lock should be corrupted");

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::SourceLockInvalid)
        );
        assert!(!report.findings.iter().any(|finding| {
            matches!(
                finding.code,
                DoctorCode::SourceLockOk
                    | DoctorCode::SourceProvenanceOk
                    | DoctorCode::SourceProvenanceMismatch
            )
        }));
    }

    #[test]
    fn run_doctor_should_point_invalid_config_to_the_editor() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store with $(shell)");
        store::init_store(store.clone(), false).expect("init should succeed");
        let config_file = store.join("config.toml");
        fs::write(&config_file, "version = ").expect("config should be corrupted");

        let report = run_doctor(&store);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.code == DoctorCode::ConfigInvalid)
            .expect("invalid config should be reported");

        assert!(finding.message.contains("line 1"));
        assert_eq!(
            finding.next_command.as_deref(),
            Some(format!("$EDITOR '{}'", config_file.display()).as_str())
        );
    }

    #[test]
    fn run_doctor_should_not_hint_editor_for_a_missing_config_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store.clone());
        fs::remove_file(&paths.config_file).expect("config should be removable");

        let report = run_doctor(&store);

        // A merely-missing file is surfaced once by `store_layout_missing` with a
        // `dalo init` hint, not as `config_invalid` with a dead-end `$EDITOR`
        // hint pointing at a path that no longer exists.
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::ConfigInvalid),
            "missing config must not be reported as config_invalid: {:?}",
            report.findings
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::StoreLayoutMissing
                    && finding.message.contains("config.toml")
                    && finding.next_command.as_deref()
                        == Some(store::dalo_command(&store, "init").as_str())),
            "missing config should be surfaced as store_layout_missing: {:?}",
            report.findings
        );
    }

    #[test]
    fn run_doctor_should_report_broken_owned_symlink() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target).expect("target should be created");
        let link = target.join("review");
        std::os::unix::fs::symlink(store.join("local/skills/missing"), &link)
            .expect("broken symlink should be created");
        write_state(&store, &target, &link, &store.join("local/skills/missing"));

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::BrokenOwnedSymlink)
        );
    }

    #[test]
    fn run_doctor_should_accept_owned_symlink_to_store_equivalent_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let store_alias = temp_dir.path().join("store-alias");
        let target = temp_dir.path().join("target");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target).expect("target should be created");
        let store_skill = store.join("local/skills/review");
        fs::create_dir_all(&store_skill).expect("skill should be created");
        std::os::unix::fs::symlink(&store, &store_alias).expect("store alias should be created");
        let link = target.join("review");
        std::os::unix::fs::symlink(store_alias.join("local/skills/review"), &link)
            .expect("owned symlink should be created");
        write_state(&store, &target, &link, &store_skill);

        let report = run_doctor(&store);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::ForeignOwnedSymlink)
        );
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::OwnedSymlinkOk)
        );
    }

    #[test]
    fn run_doctor_should_report_repointed_owned_symlink_inside_the_store() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let target = temp_dir.path().join("target");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(&target).expect("target should be created");
        let expected_skill = store.join("local/skills/review");
        let repointed_skill = store.join("local/skills/other");
        fs::create_dir_all(&expected_skill).expect("expected skill should be created");
        fs::create_dir_all(&repointed_skill).expect("repointed skill should be created");
        let link = target.join("review");
        std::os::unix::fs::symlink(&repointed_skill, &link)
            .expect("repointed symlink should be created");
        write_state(&store, &target, &link, &expected_skill);

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::OwnedSymlinkRepointed)
        );
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::OwnedSymlinkOk)
        );
    }

    #[test]
    fn command_succeeds_with_timeout_should_stop_hung_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let command = temp_dir.path().join("hang");
        fs::write(&command, "#!/bin/sh\nwhile :; do :; done\n").expect("script should be written");
        let mut permissions = fs::metadata(&command)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&command, permissions).expect("script should be executable");

        assert!(!command_succeeds_with_timeout(
            command.to_str().expect("script path should be utf-8"),
            &[],
            Duration::from_millis(10),
        ));
    }

    #[test]
    fn run_doctor_should_report_missing_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store, target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(&target, "user-owned content\n").expect("target should be rewritten");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::InstructionBlockDrift
                && finding.message.contains("missing")
                && finding
                    .next_command
                    .as_deref()
                    .is_some_and(|command| command.contains("instructions enable house-style"))
        }));
    }

    #[test]
    fn run_doctor_should_report_stale_instruction_block() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let (store, _target) = setup_enabled_instruction_pack(temp_dir.path(), "Body v1\n");
        fs::write(store.join("local/instructions/house-style.md"), "Body v2\n")
            .expect("pack should be updated");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::InstructionBlockDrift && finding.message.contains("stale")
        }));
    }

    #[test]
    fn run_doctor_should_report_invalid_approvals_without_pending_warnings() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        let team_repo = temp_dir.path().join("team-repo");
        store::init_store(store.clone(), false).expect("init should succeed");
        create_git_skill_repo(&team_repo);
        write_config_with_team_source(&store, &team_repo, false);
        fs::write(
            StorePaths::new(store.clone()).approvals_file,
            "schema_version = ",
        )
        .expect("approvals should be corrupted");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::ApprovalsInvalid
                && finding.severity == DoctorSeverity::Error
                && finding
                    .next_command
                    .as_deref()
                    .is_some_and(|command| command.starts_with("$EDITOR "))
        }));
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::PendingApproval)
        );
    }

    #[test]
    fn check_sources_should_rate_dirty_team_source_as_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team repo $(shell)");
        create_dirty_git_repo(&team_repo);
        write_config_with_dirty_sources(&store, &team_repo, None);

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Error
                && finding.message.contains("`team`")
                && finding.next_command.as_deref()
                    == Some(format!("git -C '{}' status", team_repo.display()).as_str())
        }));
    }

    #[test]
    fn check_sources_should_rate_a_missing_source_as_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        // The configured team source's checkout does not exist on disk.
        let missing_repo = temp_dir.path().join("gone-repo");
        write_config_with_dirty_sources(&store, &missing_repo, None);

        let report = run_doctor(&store);

        assert!(
            report.findings.iter().any(|finding| {
                finding.code == DoctorCode::SourceMissing
                    && finding.severity == DoctorSeverity::Error
                    && finding.message.contains("missing")
            }),
            "a missing source checkout should be reported as an error: {:?}",
            report.findings
        );
    }

    #[test]
    fn check_sources_should_rate_dirty_local_source_as_warning() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let team_repo = temp_dir.path().join("team-repo");
        let local_repo = temp_dir.path().join("local repo; echo unsafe");
        create_dirty_git_repo(&team_repo);
        create_dirty_git_repo(&local_repo);
        write_config_with_dirty_sources(&store, &team_repo, Some(&local_repo));

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::DirtySource
                && finding.severity == DoctorSeverity::Warning
                && finding.message.contains("`workspace`")
                && finding.message.contains("adopted skills must be committed")
                && finding.next_command.as_deref()
                    == Some(format!("git -C '{}' status", local_repo.display()).as_str())
        }));
    }

    #[test]
    fn doctor_should_report_unconfigured_source_operation_debris() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        let debris = store.join("sources/orphan/checkout.dalo-removing");
        fs::create_dir_all(&debris).expect("source debris should be created");

        let report = run_doctor(&store);

        assert!(report.findings.iter().any(|finding| {
            finding.code == DoctorCode::SourceStoreDebris
                && finding.severity == DoctorSeverity::Warning
                && finding
                    .message
                    .contains(debris.parent().unwrap().to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn doctor_should_classify_staging_leftovers_as_interrupted_debris() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        // A crash left a per-`{source}-{commit}` subtree in the internal staging area.
        let leftover = store.join("sources/.manifest-staging/company-deadbeef");
        fs::create_dir_all(&leftover).expect("staging leftover should be created");

        let report = run_doctor(&store);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::SourceStoreDebris
                    && finding
                        .message
                        .contains("interrupted source-operation debris")
                    && finding.message.contains("company-deadbeef")),
            "staging leftovers should be interrupted-operation debris: {:?}",
            report.findings
        );
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.message.contains("unconfigured source content")),
            "the internal staging directory must not be flagged as unconfigured source content: {:?}",
            report.findings
        );
    }

    #[test]
    fn doctor_should_not_flag_an_empty_staging_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store = temp_dir.path().join("store");
        store::init_store(store.clone(), false).expect("init should succeed");
        fs::create_dir_all(store.join("sources/.audit-staging"))
            .expect("empty staging dir should be created");

        let report = run_doctor(&store);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.code == DoctorCode::SourceStoreDebris),
            "an empty internal staging directory is not debris: {:?}",
            report.findings
        );
    }

    fn setup_enabled_instruction_pack(root: &Path, body: &str) -> (PathBuf, PathBuf) {
        let store = root.join("store");
        let target = root.join("AGENTS.md");
        store::init_store(store.clone(), false).expect("init should succeed");
        let paths = StorePaths::new(store.clone());
        fs::write(paths.local_instructions_dir.join("house-style.md"), body)
            .expect("pack should be written");
        fs::write(&target, "user-owned content\n").expect("target should be seeded");
        crate::instructions::enable_pack(&paths, "house-style", &target, false)
            .expect("pack should be enabled");
        (store, target)
    }

    fn create_dirty_git_repo(repo: &Path) {
        fs::create_dir_all(repo).expect("repo dir should be created");
        run_git(repo, &["init", "-q"]);
        fs::write(repo.join("README.md"), "tracked\n").expect("tracked file should be written");
        run_git(repo, &["add", "."]);
        run_git(
            repo,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "initial",
                "-q",
            ],
        );
        fs::write(repo.join("README.md"), "dirty\n").expect("repo should be dirtied");
    }

    fn create_git_skill_repo(repo: &Path) {
        fs::create_dir_all(repo.join("skills/review")).expect("repo skill dir should be created");
        fs::write(repo.join("skills/review/SKILL.md"), "# Review\n")
            .expect("skill should be written");
        run_git(repo, &["init", "-q"]);
        run_git(repo, &["add", "."]);
        run_git(
            repo,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-m",
                "initial",
                "-q",
            ],
        );
    }

    fn write_config_with_team_source(store: &Path, team_repo: &Path, trusted: bool) {
        use crate::config::{Settings, UserConfig};
        use crate::source::{SourceConfig, SourceKind};

        let paths = StorePaths::new(store.to_path_buf());
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources: vec![SourceConfig {
                id: "team".to_owned(),
                kind: SourceKind::Team,
                path: team_repo.to_path_buf(),
                priority: 10,
                namespace: None,
                enabled: true,
                trusted,
                url: None,
                branch: None,
                update_policy: None,
                selection: Vec::new(),
                declared_by: None,
                declared_ref: None,
            }],
            plugins: crate::config::PluginConfig::default(),
            plugin_policy: Vec::new(),
        };
        store::write_config(&paths, &config).expect("config should be written");
    }

    fn write_config_with_dirty_sources(store: &Path, team_repo: &Path, local_repo: Option<&Path>) {
        use crate::config::{Settings, UserConfig};
        use crate::source::{SourceConfig, SourceKind};

        let paths = StorePaths::new(store.to_path_buf());
        let mut sources = vec![SourceConfig {
            id: "team".to_owned(),
            kind: SourceKind::Team,
            path: team_repo.to_path_buf(),
            priority: 10,
            namespace: None,
            enabled: true,
            trusted: true,
            url: None,
            branch: None,
            update_policy: None,
            selection: Vec::new(),
            declared_by: None,
            declared_ref: None,
        }];
        if let Some(local_repo) = local_repo {
            sources.push(SourceConfig {
                id: "workspace".to_owned(),
                kind: SourceKind::Local,
                path: local_repo.to_path_buf(),
                priority: 0,
                namespace: None,
                enabled: true,
                trusted: true,
                url: None,
                branch: None,
                update_policy: None,
                selection: Vec::new(),
                declared_by: None,
                declared_ref: None,
            });
        }
        let config = UserConfig {
            version: crate::config::CONFIG_VERSION,
            settings: Settings {
                autosync: false,
                sync_interval: None,
            },
            sources,
            plugins: crate::config::PluginConfig::default(),
            plugin_policy: Vec::new(),
        };
        store::write_config(&paths, &config).expect("config should be written");
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git should run");
        assert!(status.success());
    }

    fn write_state(store: &Path, target: &Path, link: &Path, store_path: &Path) {
        let paths = StorePaths::new(store.to_path_buf());
        let mut state = store::read_state(&paths).expect("state should be readable");
        state.targets = vec![TargetState {
            id: "generic".to_owned(),
            path: target.to_path_buf(),
            canonical_path: target.to_path_buf(),
            enabled: true,
            extra: Default::default(),
        }];
        state.materialization_dirs = vec![MaterializationDirState {
            path: target.to_path_buf(),
            logical_targets: vec!["generic".to_owned()],
            extra: Default::default(),
        }];
        state.owned_skills = vec![OwnedSkillState {
            target_id: "generic".to_owned(),
            slot_name: "review".to_owned(),
            link_path: link.to_path_buf(),
            store_path: store_path.to_path_buf(),
            extra: Default::default(),
        }];
        store::write_state(&paths, &state).expect("state should be written");
    }
}
