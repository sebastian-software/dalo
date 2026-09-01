//! Portable hook semantics and executable provider-contract fixtures.
//!
//! This module deliberately does not discover, approve, install, register, or
//! execute hooks. It defines the versioned semantic boundary that later
//! adapter work must satisfy before producing native configuration.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{DaloError, DaloResult};
use crate::inventory::SourceInventory;
use crate::plugin::{HookRecord, PluginInventoryWarning, ToolRecord};
use crate::source::{SourceConfig, SourceHeadCache, SourceProvenance};
use crate::store::{self, ApprovalRecord, StorePaths};
use crate::tool::{self, ToolState};

/// Codex release against which every fixture claim was verified.
pub const CODEX_HOOK_BASELINE: &str = "0.147.0";
/// Claude Code release against which every fixture claim was verified.
pub const CLAUDE_HOOK_BASELINE: &str = "2.1.233";

/// Stable approval scope for exact hook contracts.
pub const APPROVAL_SCOPE: &str = "hook";

/// Complete inert hook inventory and approval state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookListReport {
    /// Validated hooks in deterministic identity order.
    pub hooks: Vec<HookStatusReport>,
    /// Rejected plugin packages encountered during discovery.
    pub warnings: Vec<PluginInventoryWarning>,
}

/// One hook joined with its independent approval and referenced tool state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookStatusReport {
    /// Validated hook and content-bound contract hash.
    pub hook: HookRecord,
    /// Whole plugin package hash retained as non-approval provenance.
    pub plugin_package_hash: String,
    /// Source revision and origin provenance.
    pub source_provenance: SourceProvenance,
    /// Exact approval record value.
    pub approval_value: String,
    /// Referenced tool state; only `ready` can be projected.
    pub tool_state: ToolState,
    /// Referenced same-plugin tool contract used by the dispatcher.
    pub tool: ToolRecord,
    /// Independent hook trust state.
    pub state: HookTrustState,
    /// Actionable, stable explanation.
    pub diagnostic: String,
}

/// Independent hook approval state before provider compatibility is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookTrustState {
    /// Referenced tool is not exactly approved, staged, and audited.
    ToolUnavailable,
    /// Current hook contract has never been approved.
    PendingApproval,
    /// A prior contract for this identity was approved but security inputs changed.
    HashDrift,
    /// Exact hook contract and exact referenced tool contract are ready.
    Ready,
}

/// Result of granting or revoking exact hook trust.
#[derive(Debug, Clone, Serialize)]
pub struct HookApprovalReport {
    /// Source-qualified hook identity.
    pub hook: String,
    /// Content-bound approval value.
    pub approval_value: String,
    /// `granted`, `revoked`, or `unchanged`.
    pub action: String,
    /// Whether mutation was suppressed.
    pub dry_run: bool,
}

/// Discover every valid hook without executing or installing anything.
pub fn list(paths: &StorePaths) -> DaloResult<HookListReport> {
    let config = store::read_config(paths)?;
    let approvals = store::read_approvals(paths)?;
    let inventories = tool::scan_plugin_inventories(&config.sources);
    let mut head_cache = SourceHeadCache::default();
    let tools = tool::list_from_inventories_with_head_cache(
        paths,
        &config.sources,
        &approvals.approvals,
        &inventories,
        &mut head_cache,
    );
    list_from_inventories_with_head_cache(
        paths,
        &config.sources,
        &approvals.approvals,
        &inventories,
        &tools.tools,
        &mut head_cache,
    )
}

/// Join already-scanned plugin inventories with hook and referenced tool state.
///
/// Callers that already resolved sources pass the one shared tool list, so each
/// hook lookup is an in-memory identity lookup rather than another inventory
/// scan and staged-closure audit.
pub fn list_from_inventories(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    inventories: &[SourceInventory],
    tools: &[tool::ToolStatusReport],
) -> DaloResult<HookListReport> {
    list_from_inventories_with_head_cache(
        paths,
        sources,
        approvals,
        inventories,
        tools,
        &mut SourceHeadCache::default(),
    )
}

pub(crate) fn list_from_inventories_with_head_cache(
    paths: &StorePaths,
    sources: &[SourceConfig],
    approvals: &[ApprovalRecord],
    inventories: &[SourceInventory],
    tools: &[tool::ToolStatusReport],
    head_cache: &mut SourceHeadCache,
) -> DaloResult<HookListReport> {
    let source_lock = crate::catalog::read_source_lock(paths).ok();
    let tools_by_ref = tools
        .iter()
        .map(|tool| (tool.tool.source_ref.as_str(), tool))
        .collect::<BTreeMap<_, _>>();
    let mut hooks = Vec::new();
    let mut warnings = Vec::new();
    for inventory in inventories {
        let Some(source) = sources
            .iter()
            .find(|source| source.enabled && source.id == inventory.source_id)
        else {
            continue;
        };
        warnings.extend(inventory.plugin_warnings.iter().cloned());
        if inventory
            .plugins
            .iter()
            .all(|plugin| plugin.hooks.is_empty())
        {
            continue;
        }
        let provenance = crate::source::source_provenance_with_head_cache(
            source,
            source_lock.as_ref(),
            head_cache,
        );
        for plugin in &inventory.plugins {
            for hook in &plugin.hooks {
                let tool = tools_by_ref
                    .get(hook.tool_source_ref.as_str())
                    .ok_or_else(|| DaloError::InvalidArgument {
                        reason: format!(
                            "unknown tool `{}`; use `dalo tool list` and an exact `<source>:<plugin>#tool:<id>` identity",
                            hook.tool_source_ref
                        ),
                    })?;
                hooks.push(status_for(
                    hook.clone(),
                    plugin.package_hash.clone(),
                    provenance.clone(),
                    (*tool).clone(),
                    approvals,
                ));
            }
        }
    }
    hooks.sort_by(|left, right| left.hook.source_ref.cmp(&right.hook.source_ref));
    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(HookListReport { hooks, warnings })
}

/// Find one exact source-qualified hook.
pub fn show(paths: &StorePaths, value: &str) -> DaloResult<HookStatusReport> {
    list(paths)?
        .hooks
        .into_iter()
        .find(|candidate| candidate.hook.source_ref == value)
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!(
                "unknown hook `{value}`; use `dalo hook list` and an exact `<source>:<plugin>#hook:<id>` identity"
            ),
        })
}

/// Grant exact hook trust only after its separately approved tool is ready.
pub fn approve(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<HookApprovalReport> {
    let status = show(paths, value)?;
    if status.tool_state != ToolState::Ready {
        return Err(DaloError::StateError {
            reason: format!(
                "hook `{}` references tool `{}` in state {:?}; approve and stage the exact tool first",
                status.hook.source_ref, status.hook.tool_source_ref, status.tool_state
            ),
        });
    }
    let mut approvals = store::read_approvals(paths)?;
    let record = ApprovalRecord {
        scope: APPROVAL_SCOPE.to_owned(),
        value: status.approval_value.clone(),
    };
    let exists = approvals.approvals.contains(&record);
    if !exists && !dry_run {
        approvals.approvals.push(record);
        approvals.approvals.sort_by(|left, right| {
            left.scope
                .cmp(&right.scope)
                .then(left.value.cmp(&right.value))
        });
        store::write_approvals(paths, &approvals)?;
    }
    Ok(HookApprovalReport {
        hook: status.hook.source_ref,
        approval_value: status.approval_value,
        action: if exists { "unchanged" } else { "granted" }.to_owned(),
        dry_run,
    })
}

/// Revoke every approved contract hash for one exact hook identity.
pub fn revoke(paths: &StorePaths, value: &str, dry_run: bool) -> DaloResult<HookApprovalReport> {
    validate_identity_shape(value)?;
    let mut approvals = store::read_approvals(paths)?;
    let prefix = format!("{value}@sha256:");
    let mut removed = None;
    approvals.approvals.retain(|record| {
        let matches = record.scope == APPROVAL_SCOPE && record.value.starts_with(&prefix);
        if matches {
            removed = Some(record.value.clone());
        }
        !matches
    });
    let changed = removed.is_some();
    if changed && !dry_run {
        store::write_approvals(paths, &approvals)?;
    }
    Ok(HookApprovalReport {
        hook: value.to_owned(),
        approval_value: removed.unwrap_or(prefix),
        action: if changed { "revoked" } else { "unchanged" }.to_owned(),
        dry_run,
    })
}

fn status_for(
    hook: HookRecord,
    plugin_package_hash: String,
    source_provenance: SourceProvenance,
    tool: tool::ToolStatusReport,
    approvals: &[ApprovalRecord],
) -> HookStatusReport {
    let tool_state = tool.state;
    let approval_value = format!("{}@sha256:{}", hook.source_ref, hook.contract_hash);
    let exact = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value == approval_value);
    let prefix = format!("{}@sha256:", hook.source_ref);
    let prior = approvals
        .iter()
        .any(|record| record.scope == APPROVAL_SCOPE && record.value.starts_with(&prefix));
    let (state, diagnostic) = if tool_state != ToolState::Ready {
        (
            HookTrustState::ToolUnavailable,
            "referenced tool is not exactly approved, staged, and audited".to_owned(),
        )
    } else if exact {
        (
            HookTrustState::Ready,
            "exact hook contract and referenced tool contract are approved".to_owned(),
        )
    } else if prior {
        (
            HookTrustState::HashDrift,
            "security-relevant hook or referenced tool contract changed".to_owned(),
        )
    } else {
        (
            HookTrustState::PendingApproval,
            format!("run `dalo approve hook {}`", hook.source_ref),
        )
    };
    HookStatusReport {
        hook,
        plugin_package_hash,
        source_provenance,
        approval_value,
        tool_state,
        tool: tool.tool,
        state,
        diagnostic,
    }
}

fn validate_identity_shape(value: &str) -> DaloResult<()> {
    if value
        .split_once("#hook:")
        .is_some_and(|(plugin, hook)| plugin.contains(':') && !hook.is_empty())
    {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: "hook values must use `<source>:<plugin>#hook:<id>`".to_owned(),
        })
    }
}

/// Provider adapters covered by the first hook contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookProvider {
    /// OpenAI Codex CLI.
    Codex,
    /// Anthropic Claude Code.
    Claude,
}

impl HookProvider {
    /// Exact release baseline owned by the executable fixtures.
    #[must_use]
    pub const fn baseline(self) -> &'static str {
        match self {
            Self::Codex => CODEX_HOOK_BASELINE,
            Self::Claude => CLAUDE_HOOK_BASELINE,
        }
    }
}

/// Portable semantic subject, independent of provider event names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookSubject {
    /// Main-session lifecycle.
    Session,
    /// User prompt submission.
    UserPrompt,
    /// One provider tool call on a covered hook path.
    ToolCall,
    /// Main-agent workflow completion.
    Workflow,
}

/// Timing relative to the semantic subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPhase {
    /// Before a prompt is submitted or tool call executes.
    Before,
    /// After a tool call has produced a result; side effects already happened.
    After,
    /// Final main-session termination observation.
    End,
    /// Agent-turn completion attempt that may be continued.
    CompletionAttempt,
}

/// Requested portable effect. Event identity never implies an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEffect {
    /// Observe the event without influencing provider behavior.
    Observe,
    /// Add bounded model-visible context.
    AddContext,
    /// Permit or deny one covered pre-action event; provider permissions remain.
    AllowDeny,
    /// Replace the complete input object before the action.
    RewriteInput,
    /// Replace the model-facing result after the action, never its side effects.
    ReplaceOutput,
    /// Request another agent turn at a completion attempt.
    ContinueWorkflow,
}

impl HookEffect {
    /// Whether the effect claims enforcement over an event that has not happened.
    #[must_use]
    pub const fn is_pre_action_enforcement(self) -> bool {
        matches!(self, Self::AllowDeny | Self::RewriteInput)
    }
}

/// Authored component strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRequirement {
    /// Missing semantics block the plugin-target projection.
    Required,
    /// Missing semantics may be omitted only through an authored fallback.
    Optional,
}

/// Failure behavior requested from a future Dalo dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailurePolicy {
    /// Continue the provider action and report the hook failure.
    FailOpen,
    /// Deny or retain control when the hook cannot return a valid decision.
    FailClosed,
    /// The event already happened; report failure without claiming control.
    Report,
}

/// Retry policy accepted by hook descriptor version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookRetryPolicy {
    /// Never replay a handler whose idempotency is unknown.
    Never,
}

/// Where bounded redacted hook failures are surfaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookErrorVisibility {
    /// Show the failure to the user only.
    User,
    /// Show the failure to the user and make it model-visible.
    ModelAndUser,
}

/// Authored optional fallback accepted by descriptor version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFallback {
    /// Intentionally omit unsupported optional behavior.
    Omit,
}

/// Exact event field admitted by hook descriptor version 1 bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HookEventField {
    /// Opaque provider session identity.
    #[serde(rename = "session.id")]
    SessionId,
    /// Absolute normalized session working directory.
    #[serde(rename = "session.cwd")]
    SessionCwd,
    /// Normalized informational permission mode.
    #[serde(rename = "session.permission_mode")]
    SessionPermissionMode,
    /// Root or subagent actor kind.
    #[serde(rename = "actor.kind")]
    ActorKind,
    /// Optional opaque subagent identity.
    #[serde(rename = "actor.id")]
    ActorId,
    /// Optional absolute provider transcript path.
    #[serde(rename = "transcript.path")]
    TranscriptPath,
    /// Normalized final session reason.
    #[serde(rename = "session.end_reason")]
    SessionEndReason,
    /// Submitted user prompt.
    #[serde(rename = "prompt.text")]
    PromptText,
    /// Opaque provider tool-call identity.
    #[serde(rename = "tool.call_id")]
    ToolCallId,
    /// Provider tool identifier.
    #[serde(rename = "tool.name")]
    ToolName,
    /// Whether the completion hook already continued the workflow.
    #[serde(rename = "workflow.already_continued")]
    WorkflowAlreadyContinued,
    /// Optional last assistant message at a completion attempt.
    #[serde(rename = "workflow.last_message")]
    WorkflowLastMessage,
}

impl HookEventField {
    /// Stable field spelling used by descriptor hashes and dispatcher payloads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionId => "session.id",
            Self::SessionCwd => "session.cwd",
            Self::SessionPermissionMode => "session.permission_mode",
            Self::ActorKind => "actor.kind",
            Self::ActorId => "actor.id",
            Self::TranscriptPath => "transcript.path",
            Self::SessionEndReason => "session.end_reason",
            Self::PromptText => "prompt.text",
            Self::ToolCallId => "tool.call_id",
            Self::ToolName => "tool.name",
            Self::WorkflowAlreadyContinued => "workflow.already_continued",
            Self::WorkflowLastMessage => "workflow.last_message",
        }
    }

    /// Primitive input type required from the referenced local tool.
    #[must_use]
    pub const fn input_type(self) -> HookBindingType {
        match self {
            Self::SessionCwd | Self::TranscriptPath => HookBindingType::Path,
            Self::WorkflowAlreadyContinued => HookBindingType::Boolean,
            _ => HookBindingType::String,
        }
    }
}

/// Provider-independent primitive type used to validate a hook binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookBindingType {
    /// UTF-8 scalar text.
    String,
    /// Absolute normalized filesystem path.
    Path,
    /// Boolean scalar.
    Boolean,
}

/// One event-field to local-tool input binding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookBindingV1 {
    /// Referenced local-tool named input.
    pub input: String,
    /// Typed portable event field.
    pub field: HookEventField,
}

/// Closed portable matcher. Empty tool names match every covered tool call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookMatcherV1 {
    /// Exact provider tool names, combined as an escaped adapter regex.
    #[serde(default)]
    pub tool_names: Vec<String>,
}

/// Scope over which a controlling result is authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookBlockingScope {
    /// Only the one native event instance matched by this descriptor.
    MatchedEvent,
}

/// Exact closed portable hook descriptor schema version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDescriptorV1 {
    /// Independent descriptor schema version; must equal one.
    pub schema_version: u32,
    /// Plugin-local lower-kebab identity.
    pub id: String,
    /// Same-plugin local tool ID owned by RFC 0005/#499.
    pub tool: String,
    /// Semantic subject.
    pub subject: HookSubject,
    /// Timing relative to the subject.
    pub phase: HookPhase,
    /// Requested effect.
    pub effect: HookEffect,
    /// Required or optional behavior.
    pub requirement: HookRequirement,
    /// Portable dispatcher timeout in milliseconds.
    pub timeout_ms: u32,
    /// Behavior when the handler fails or returns malformed output.
    pub failure_policy: HookFailurePolicy,
    /// Bounded retry policy.
    pub retry: HookRetryPolicy,
    /// Failure audience.
    pub error_visibility: HookErrorVisibility,
    /// Portable exact-name filter.
    #[serde(default)]
    pub matcher: HookMatcherV1,
    /// Typed event fields mapped to the referenced tool's named inputs.
    #[serde(default)]
    pub bindings: Vec<HookBindingV1>,
    /// Bounded scope for controlling decisions.
    pub blocking_scope: HookBlockingScope,
    /// Explicit optional fallback; absent for required hooks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<HookFallback>,
}

/// Validate the exact hook descriptor without installing or executing it.
pub fn validate_descriptor(descriptor: &HookDescriptorV1) -> Result<(), String> {
    if descriptor.schema_version != 1 {
        return Err(format!(
            "unsupported hook descriptor schema {} (supported: 1)",
            descriptor.schema_version
        ));
    }
    if !is_local_id(&descriptor.id) || !is_local_id(&descriptor.tool) {
        return Err("hook.id and hook.tool must use lower kebab-case".to_owned());
    }
    if !(100..=120_000).contains(&descriptor.timeout_ms) {
        return Err("hook.timeout_ms must be between 100 and 120000".to_owned());
    }
    if descriptor.matcher.tool_names.len() > 256 || descriptor.bindings.len() > 256 {
        return Err("hook matcher and bindings accept at most 256 entries".to_owned());
    }
    let mut tool_names = descriptor.matcher.tool_names.clone();
    tool_names.sort();
    tool_names.dedup();
    if tool_names.len() != descriptor.matcher.tool_names.len()
        || tool_names.iter().any(|name| {
            name.is_empty()
                || name.len() > 512
                || name.chars().any(|character| character.is_control())
        })
    {
        return Err("hook matcher tool_names must be unique bounded exact names".to_owned());
    }
    if descriptor.subject != HookSubject::ToolCall && !tool_names.is_empty() {
        return Err("hook matcher tool_names are valid only for tool_call events".to_owned());
    }
    let mut inputs = descriptor
        .bindings
        .iter()
        .map(|binding| binding.input.as_str())
        .collect::<Vec<_>>();
    inputs.sort_unstable();
    if inputs.windows(2).any(|pair| pair[0] == pair[1])
        || descriptor
            .bindings
            .iter()
            .any(|binding| !is_input_name(&binding.input))
    {
        return Err("hook binding inputs must be unique lower snake-case values".to_owned());
    }
    for binding in &descriptor.bindings {
        if !field_available(descriptor.subject, descriptor.phase, binding.field) {
            return Err(format!(
                "hook field `{}` is unavailable for this event",
                binding.field.as_str()
            ));
        }
    }
    if !effect_supported(
        HookProvider::Claude,
        descriptor.subject,
        descriptor.phase,
        descriptor.effect,
    ) && !effect_supported(
        HookProvider::Codex,
        descriptor.subject,
        descriptor.phase,
        descriptor.effect,
    ) {
        return Err("hook subject, phase, and effect combination is invalid".to_owned());
    }
    match descriptor.requirement {
        HookRequirement::Required if descriptor.fallback.is_some() => {
            return Err("required hooks must not declare a weakening fallback".to_owned());
        }
        HookRequirement::Optional if descriptor.fallback != Some(HookFallback::Omit) => {
            return Err("optional hooks must explicitly author fallback = `omit`".to_owned());
        }
        _ => {}
    }
    let controlling = (descriptor.phase == HookPhase::Before
        && descriptor.effect.is_pre_action_enforcement())
        || (descriptor.phase == HookPhase::CompletionAttempt
            && descriptor.effect == HookEffect::ContinueWorkflow);
    if descriptor.failure_policy == HookFailurePolicy::FailClosed && !controlling {
        return Err("fail_closed is valid only for pre-action or completion control".to_owned());
    }
    if matches!(
        descriptor.effect,
        HookEffect::Observe | HookEffect::ReplaceOutput
    ) && descriptor.failure_policy != HookFailurePolicy::Report
    {
        return Err(
            "observational and post-action replacement effects must report failures".to_owned(),
        );
    }
    Ok(())
}

fn field_available(subject: HookSubject, phase: HookPhase, field: HookEventField) -> bool {
    match field {
        HookEventField::SessionId | HookEventField::SessionCwd | HookEventField::ActorKind => true,
        HookEventField::SessionPermissionMode => phase != HookPhase::End,
        HookEventField::ActorId => true,
        HookEventField::TranscriptPath => {
            matches!(subject, HookSubject::Session | HookSubject::Workflow)
        }
        HookEventField::SessionEndReason => {
            subject == HookSubject::Session && phase == HookPhase::End
        }
        HookEventField::PromptText => subject == HookSubject::UserPrompt,
        HookEventField::ToolCallId | HookEventField::ToolName => subject == HookSubject::ToolCall,
        HookEventField::WorkflowAlreadyContinued | HookEventField::WorkflowLastMessage => {
            subject == HookSubject::Workflow && phase == HookPhase::CompletionAttempt
        }
    }
}

fn is_local_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn is_input_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !value.ends_with('_')
        && !value.contains("__")
}

/// Compute the complete hook-specific approval contract.
#[must_use]
pub fn contract_hash(
    source_ref: &str,
    descriptor: &HookDescriptorV1,
    tool_source_ref: &str,
    tool_contract_hash: &str,
) -> String {
    let mut hash = Sha256::new();
    hash.update(b"dalo-hook-contract-v1\0");
    hash_value(&mut hash, source_ref);
    hash_value(&mut hash, tool_source_ref);
    hash_value(&mut hash, tool_contract_hash);
    hash_value(&mut hash, &descriptor.schema_version.to_string());
    hash_value(&mut hash, &format!("{:?}", descriptor.subject));
    hash_value(&mut hash, &format!("{:?}", descriptor.phase));
    hash_value(&mut hash, &format!("{:?}", descriptor.effect));
    hash_value(&mut hash, &format!("{:?}", descriptor.requirement));
    hash_value(&mut hash, &descriptor.timeout_ms.to_string());
    hash_value(&mut hash, &format!("{:?}", descriptor.failure_policy));
    hash_value(&mut hash, &format!("{:?}", descriptor.retry));
    hash_value(&mut hash, &format!("{:?}", descriptor.error_visibility));
    hash_value(&mut hash, &format!("{:?}", descriptor.blocking_scope));
    hash_value(&mut hash, &format!("{:?}", descriptor.fallback));
    let mut tool_names = descriptor.matcher.tool_names.clone();
    tool_names.sort();
    for name in tool_names {
        hash_value(&mut hash, &format!("matcher:{name}"));
    }
    let mut bindings = descriptor.bindings.clone();
    bindings.sort();
    for binding in bindings {
        hash_value(
            &mut hash,
            &format!("binding:{}:{}", binding.input, binding.field.as_str()),
        );
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_value(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value.as_bytes());
}

/// Provider mapping quality retained by plans and fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookCompatibility {
    /// Provider semantics directly match the portable contract.
    Exact,
    /// A deterministic adapter translation preserves the contract.
    Mapped,
    /// Useful observation or context exists but cannot satisfy enforcement.
    GuidanceOnly,
    /// Optional semantics cannot be represented.
    Unsupported,
    /// Required semantics or a safety boundary cannot be represented.
    Blocked,
    /// Provider version differs from the verified fixture baseline.
    UnverifiedVersion,
}

/// Where a provider tool call travels relative to native hook coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPathCoverage {
    /// Documented local function-tool path covered by pre/post hooks.
    CoveredLocal,
    /// Provider-hosted tool outside Codex's local function hook path.
    Hosted,
    /// Specialized or explicitly uncovered provider path.
    Uncovered,
    /// Scenario does not concern a tool call.
    NotApplicable,
}

/// Session permission mode carried as data, never treated as hook authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixturePermissionMode {
    /// Normal interactive/default permission behavior.
    Default,
    /// Provider bypass mode; Dalo hook denials must still retain their meaning.
    BypassPermissions,
    /// Planning mode.
    Plan,
}

/// Actor for a fixture event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookActor {
    /// Main/root agent and main session.
    Root,
    /// Provider subagent.
    Subagent,
}

/// Native hook outcome supplied to the fixture adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureBehavior {
    /// Successful observation with no control output.
    Observe,
    /// Successful allow/abstain result.
    Allow,
    /// Successful denial.
    Deny,
    /// Valid complete input rewrite.
    Rewrite,
    /// Valid complete output replacement.
    Replace,
    /// Valid workflow continuation request.
    Continue,
    /// Handler exceeded the configured timeout.
    Timeout,
    /// Handler returned output that failed its event schema.
    MalformedOutput,
}

/// One executable adapter-contract fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterFixture {
    /// Stable matrix/fixture identifier.
    pub id: String,
    /// Provider under test.
    pub provider: HookProvider,
    /// Exact provider version against which the claim was verified.
    pub provider_version: String,
    /// Portable subject.
    pub subject: HookSubject,
    /// Portable timing.
    pub phase: HookPhase,
    /// Requested effect.
    pub effect: HookEffect,
    /// Required or optional behavior.
    pub requirement: HookRequirement,
    /// Authored failure policy.
    pub failure_policy: HookFailurePolicy,
    /// Whether native hooks are enabled for this run.
    pub hooks_enabled: bool,
    /// Whether policy permits plugin/project hooks rather than managed hooks only.
    pub plugin_hooks_allowed: bool,
    /// Main agent or subagent event.
    pub actor: HookActor,
    /// Tool-path coverage class.
    pub tool_path: ToolPathCoverage,
    /// Permission mode exposed in the native payload.
    pub permission_mode: FixturePermissionMode,
    /// Simulated native handler behavior.
    pub behavior: FixtureBehavior,
    /// Expected adapter compatibility.
    pub expected: HookCompatibility,
}

/// Result of evaluating one provider fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterFixtureResult {
    /// Fixture identifier.
    pub id: String,
    /// Native provider event selected by the adapter, if supported.
    pub native_event: Option<String>,
    /// Effective compatibility.
    pub compatibility: HookCompatibility,
    /// Whether a required pre-action enforcement claim is actually satisfied.
    pub enforcement_satisfied: bool,
    /// Stable explanation for matrix and plan output.
    pub reason: String,
}

/// Parse a fixture document with a closed schema.
pub fn parse_fixtures(document: &str) -> Result<Vec<AdapterFixture>, serde_json::Error> {
    serde_json::from_str(document)
}

/// Evaluate a fixture without running a provider or hook process.
#[must_use]
pub fn evaluate_fixture(fixture: &AdapterFixture) -> AdapterFixtureResult {
    let native_event =
        native_event(fixture.provider, fixture.subject, fixture.phase).map(str::to_owned);
    let (compatibility, reason) = if fixture.provider_version != fixture.provider.baseline() {
        (
            HookCompatibility::UnverifiedVersion,
            format!(
                "provider {} differs from verified baseline {}",
                fixture.provider_version,
                fixture.provider.baseline()
            ),
        )
    } else if !fixture.hooks_enabled {
        unavailable(fixture, "hooks are disabled")
    } else if !fixture.plugin_hooks_allowed {
        unavailable(fixture, "policy allows managed hooks only")
    } else if native_event.is_none() {
        unavailable(fixture, "portable event/effect has no provider mapping")
    } else if fixture.subject == HookSubject::Session && fixture.actor == HookActor::Subagent {
        unavailable(fixture, "session.end is main-session-only")
    } else if fixture.subject == HookSubject::ToolCall
        && fixture.tool_path != ToolPathCoverage::CoveredLocal
    {
        unavailable(
            fixture,
            "tool path is outside verified native hook coverage",
        )
    } else if !effect_supported(
        fixture.provider,
        fixture.subject,
        fixture.phase,
        fixture.effect,
    ) {
        unavailable(
            fixture,
            "requested effect is not valid for the portable event",
        )
    } else if matches!(
        fixture.behavior,
        FixtureBehavior::Timeout | FixtureBehavior::MalformedOutput
    ) {
        failure_compatibility(fixture)
    } else {
        (
            base_compatibility(fixture.subject, fixture.phase),
            "fixture preserves the portable event and effect".to_owned(),
        )
    };
    let enforcement_satisfied = fixture.effect.is_pre_action_enforcement()
        && matches!(
            compatibility,
            HookCompatibility::Exact | HookCompatibility::Mapped
        );
    AdapterFixtureResult {
        id: fixture.id.clone(),
        native_event,
        compatibility,
        enforcement_satisfied,
        reason,
    }
}

fn unavailable(fixture: &AdapterFixture, reason: &str) -> (HookCompatibility, String) {
    let compatibility = if fixture.requirement == HookRequirement::Required {
        HookCompatibility::Blocked
    } else {
        HookCompatibility::Unsupported
    };
    (compatibility, reason.to_owned())
}

fn failure_compatibility(fixture: &AdapterFixture) -> (HookCompatibility, String) {
    let failure = match fixture.behavior {
        FixtureBehavior::Timeout => "handler timed out",
        FixtureBehavior::MalformedOutput => "handler output is malformed",
        _ => unreachable!("caller filters successful fixture behavior"),
    };
    if fixture.effect.is_pre_action_enforcement()
        && fixture.failure_policy == HookFailurePolicy::FailClosed
    {
        unavailable(
            fixture,
            &format!("{failure}; native command hooks fail open without a Dalo dispatcher"),
        )
    } else if fixture.requirement == HookRequirement::Required {
        (
            HookCompatibility::Blocked,
            format!("{failure}; required behavior was not delivered"),
        )
    } else {
        (
            HookCompatibility::GuidanceOnly,
            format!("{failure}; provider action continues and failure remains visible"),
        )
    }
}

fn base_compatibility(subject: HookSubject, phase: HookPhase) -> HookCompatibility {
    if subject == HookSubject::Session && phase == HookPhase::End {
        HookCompatibility::Exact
    } else {
        HookCompatibility::Mapped
    }
}

fn effect_supported(
    provider: HookProvider,
    subject: HookSubject,
    phase: HookPhase,
    effect: HookEffect,
) -> bool {
    match (subject, phase) {
        (HookSubject::Session, HookPhase::End) => effect == HookEffect::Observe,
        (HookSubject::Workflow, HookPhase::CompletionAttempt) => {
            matches!(effect, HookEffect::Observe | HookEffect::ContinueWorkflow)
        }
        (HookSubject::UserPrompt, HookPhase::Before) => matches!(
            effect,
            HookEffect::Observe | HookEffect::AddContext | HookEffect::AllowDeny
        ),
        (HookSubject::ToolCall, HookPhase::Before) => matches!(
            effect,
            HookEffect::Observe
                | HookEffect::AddContext
                | HookEffect::AllowDeny
                | HookEffect::RewriteInput
        ),
        (HookSubject::ToolCall, HookPhase::After) => {
            matches!(effect, HookEffect::Observe | HookEffect::AddContext)
                || (provider == HookProvider::Claude && effect == HookEffect::ReplaceOutput)
        }
        _ => false,
    }
}

/// Whether one verified provider adapter preserves this descriptor's event/effect pair.
#[must_use]
pub fn provider_supports_descriptor(provider: HookProvider, descriptor: &HookDescriptorV1) -> bool {
    native_event(provider, descriptor.subject, descriptor.phase).is_some()
        && effect_supported(
            provider,
            descriptor.subject,
            descriptor.phase,
            descriptor.effect,
        )
}

fn native_event(
    provider: HookProvider,
    subject: HookSubject,
    phase: HookPhase,
) -> Option<&'static str> {
    match (provider, subject, phase) {
        (HookProvider::Codex, HookSubject::Session, HookPhase::End)
        | (HookProvider::Claude, HookSubject::Session, HookPhase::End) => Some("SessionEnd"),
        (HookProvider::Codex, HookSubject::Workflow, HookPhase::CompletionAttempt)
        | (HookProvider::Claude, HookSubject::Workflow, HookPhase::CompletionAttempt) => {
            Some("Stop")
        }
        (HookProvider::Codex, HookSubject::UserPrompt, HookPhase::Before)
        | (HookProvider::Claude, HookSubject::UserPrompt, HookPhase::Before) => {
            Some("UserPromptSubmit")
        }
        (HookProvider::Codex, HookSubject::ToolCall, HookPhase::Before)
        | (HookProvider::Claude, HookSubject::ToolCall, HookPhase::Before) => Some("PreToolUse"),
        (HookProvider::Codex, HookSubject::ToolCall, HookPhase::After) => Some("PostToolUse"),
        (HookProvider::Claude, HookSubject::ToolCall, HookPhase::After) => {
            Some("PostToolUse|PostToolUseFailure")
        }
        _ => None,
    }
}

/// Typed output from one portable hook, tagged for deterministic composition.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortableHookResult {
    /// Source-qualified hook identity.
    pub hook: String,
    /// Effect-specific output.
    pub output: PortableHookOutput,
}

/// Bounded effect output vocabulary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortableHookOutput {
    /// No control output.
    Observe,
    /// Model-visible context, capped by validation outside composition.
    AddContext {
        /// Context text, validated against the portable size bound.
        context: String,
    },
    /// Explicit abstention.
    Abstain,
    /// Permit this hook's policy check; native permissions still apply.
    Allow,
    /// Deny the pending action.
    Deny {
        /// User-visible and model-visible denial explanation.
        reason: String,
    },
    /// Complete replacement input object.
    RewriteInput {
        /// Complete replacement input object.
        input: serde_json::Value,
    },
    /// Complete replacement model-facing output object.
    ReplaceOutput {
        /// Complete replacement model-facing output.
        output: serde_json::Value,
    },
    /// Request another workflow turn.
    ContinueWorkflow {
        /// Prompt text explaining the requested continuation.
        reason: String,
    },
}

/// Deterministically composed result for all matching portable hooks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComposedHookOutcome {
    /// Denial reasons in source-qualified hook order.
    pub denials: Vec<String>,
    /// Context chunks in source-qualified hook order.
    pub context: Vec<String>,
    /// Unique rewrite or replacement value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Whether the workflow must continue.
    pub continue_workflow: bool,
    /// Composition conflicts that fail closed for controlling effects.
    pub conflicts: Vec<String>,
}

/// Provider-native hook entries plus the immutable dispatcher manifest.
#[derive(Debug, Clone, Serialize)]
pub struct NativeHookProjection {
    /// Provider receiving the native entries.
    pub provider: HookProvider,
    /// Exact verified provider baseline.
    pub provider_version: String,
    /// Hash-addressed dispatcher projection identity.
    pub fingerprint: String,
    /// Number of independently approved portable hooks represented.
    pub portable_hooks: usize,
    /// Native hook groups keyed by provider event name.
    pub hooks: BTreeMap<String, Vec<Value>>,
    /// Closed dispatcher manifest stored under the Dalo store.
    pub dispatcher_manifest: Value,
}

/// Compile approved hook contracts without writing or running anything.
pub fn compile_native_projection(
    paths: &StorePaths,
    provider: HookProvider,
    provider_version: &str,
    dalo_executable: &Path,
    statuses: &[HookStatusReport],
) -> DaloResult<NativeHookProjection> {
    if provider_version != provider.baseline() {
        return Err(DaloError::StateError {
            reason: format!(
                "{provider:?} version `{provider_version}` is unverified; expected `{}`",
                provider.baseline()
            ),
        });
    }
    let mut approved = statuses.iter().collect::<Vec<_>>();
    approved.sort_by(|left, right| left.hook.source_ref.cmp(&right.hook.source_ref));
    if let Some(status) = approved
        .iter()
        .find(|status| status.state != HookTrustState::Ready)
    {
        return Err(DaloError::StateError {
            reason: format!(
                "hook `{}` is not projectable: {}",
                status.hook.source_ref, status.diagnostic
            ),
        });
    }
    let mut grouped: BTreeMap<(String, Option<String>), Vec<&HookStatusReport>> = BTreeMap::new();
    for status in &approved {
        let descriptor = &status.hook.descriptor;
        if !effect_supported(
            provider,
            descriptor.subject,
            descriptor.phase,
            descriptor.effect,
        ) {
            if descriptor.requirement == HookRequirement::Optional
                && descriptor.fallback == Some(HookFallback::Omit)
            {
                continue;
            }
            return Err(DaloError::StateError {
                reason: format!(
                    "hook `{}` has no verified {provider:?} mapping",
                    status.hook.source_ref
                ),
            });
        }
        let matcher = exact_matcher(&descriptor.matcher.tool_names);
        for event in native_event(provider, descriptor.subject, descriptor.phase)
            .expect("supported descriptor has a native event")
            .split('|')
        {
            grouped
                .entry((event.to_owned(), matcher.clone()))
                .or_default()
                .push(status);
        }
    }
    let represented = grouped
        .values()
        .flat_map(|statuses| {
            statuses
                .iter()
                .map(|status| status.hook.source_ref.as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let manifest_hooks = approved
        .iter()
        .filter(|status| represented.contains(status.hook.source_ref.as_str()))
        .map(|status| {
            let descriptor = &status.hook.descriptor;
            json!({
                "identity": status.hook.source_ref,
                "contract_hash": status.hook.contract_hash,
                "tool": status.hook.tool_source_ref,
                "tool_contract_hash": status.hook.tool_contract_hash,
                "tool_root": paths.tools_dir.join("sha256").join(&status.hook.tool_contract_hash),
                "tool_contract": status.tool,
                "descriptor": descriptor,
            })
        })
        .collect::<Vec<_>>();
    let groups = grouped
        .iter()
        .enumerate()
        .map(|(index, ((event, matcher), statuses))| {
            json!({
                "id": format!("group-{index:04}"),
                "event": event,
                "matcher": matcher,
                "hooks": statuses
                    .iter()
                    .map(|status| status.hook.source_ref.as_str())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let dispatcher_manifest = json!({
        "schema_version": 1,
        "provider": provider,
        "provider_version": provider_version,
        "hooks": manifest_hooks,
        "groups": groups,
    });
    let manifest_bytes = serde_json::to_vec(&dispatcher_manifest)?;
    let fingerprint = Sha256::digest(&manifest_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut hooks: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for (index, ((event, matcher), event_hooks)) in grouped.into_iter().enumerate() {
        let timeout_ms = event_hooks
            .iter()
            .map(|status| status.hook.descriptor.timeout_ms)
            .min()
            .unwrap_or(100);
        let timeout_seconds = timeout_ms.div_ceil(1_000).max(1);
        let arguments = vec![
            "--store".to_owned(),
            paths.root.to_string_lossy().into_owned(),
            "hook".to_owned(),
            "dispatch".to_owned(),
            "--provider".to_owned(),
            match provider {
                HookProvider::Codex => "codex".to_owned(),
                HookProvider::Claude => "claude".to_owned(),
            },
            "--projection".to_owned(),
            fingerprint.clone(),
            "--event".to_owned(),
            event.clone(),
            "--group".to_owned(),
            format!("group-{index:04}"),
        ];
        let handler = match provider {
            HookProvider::Claude => json!({
                "type": "command",
                "command": dalo_executable,
                "args": arguments,
                "timeout": timeout_seconds,
            }),
            HookProvider::Codex => {
                let mut posix = vec![crate::error::shell_quote_path(dalo_executable)];
                posix.extend(arguments.iter().map(|argument| shell_quote_data(argument)));
                json!({
                    "type": "command",
                    "command": posix.join(" "),
                    "commandWindows": powershell_encoded_command(dalo_executable, &arguments),
                    "timeout": timeout_seconds,
                })
            }
        };
        let mut group = serde_json::Map::new();
        if let Some(matcher) = matcher {
            group.insert("matcher".to_owned(), Value::String(matcher));
        }
        group.insert("hooks".to_owned(), Value::Array(vec![handler]));
        hooks.entry(event).or_default().push(Value::Object(group));
    }
    Ok(NativeHookProjection {
        provider,
        provider_version: provider_version.to_owned(),
        fingerprint,
        portable_hooks: represented.len(),
        hooks,
        dispatcher_manifest,
    })
}

fn exact_matcher(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let mut names = names.to_vec();
    names.sort();
    Some(format!(
        "^(?:{})$",
        names
            .iter()
            .map(|name| regex_escape(name))
            .collect::<Vec<_>>()
            .join("|")
    ))
}

fn regex_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(
            character,
            '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn shell_quote_data(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn powershell_encoded_command(executable: &Path, arguments: &[String]) -> String {
    let mut script = format!("& '{}'", executable.to_string_lossy().replace('\'', "''"));
    for argument in arguments {
        script.push_str(" '");
        script.push_str(&argument.replace('\'', "''"));
        script.push('\'');
    }
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        base64(&bytes)
    )
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(third & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

/// Compose matching outputs independently of provider completion order.
#[must_use]
pub fn compose_results(effect: HookEffect, results: &[PortableHookResult]) -> ComposedHookOutcome {
    let mut ordered = results.to_vec();
    ordered.sort_by(|left, right| left.hook.cmp(&right.hook));
    let mut denials = Vec::new();
    let mut context = Vec::new();
    let mut values = Vec::new();
    let mut continue_workflow = false;
    let mut conflicts = Vec::new();
    for result in ordered {
        match result.output {
            PortableHookOutput::Observe
            | PortableHookOutput::Abstain
            | PortableHookOutput::Allow => {}
            PortableHookOutput::AddContext { context: value }
                if effect == HookEffect::AddContext =>
            {
                context.push(value)
            }
            PortableHookOutput::Deny { reason } if effect == HookEffect::AllowDeny => {
                denials.push(reason);
            }
            PortableHookOutput::RewriteInput { input } if effect == HookEffect::RewriteInput => {
                values.push(input);
            }
            PortableHookOutput::ReplaceOutput { output } if effect == HookEffect::ReplaceOutput => {
                values.push(output)
            }
            PortableHookOutput::ContinueWorkflow { reason }
                if effect == HookEffect::ContinueWorkflow =>
            {
                continue_workflow = true;
                context.push(reason);
            }
            _ => conflicts.push(format!(
                "hook `{}` returned an output incompatible with {effect:?}",
                result.hook
            )),
        }
    }
    let value = values.first().cloned();
    let value = if values
        .iter()
        .skip(1)
        .any(|candidate| Some(candidate) != value.as_ref())
    {
        conflicts.push("multiple hooks returned divergent replacement values".to_owned());
        None
    } else {
        value
    };
    ComposedHookOutcome {
        denials,
        context,
        value,
        continue_workflow,
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn valid_descriptor() -> HookDescriptorV1 {
        HookDescriptorV1 {
            schema_version: 1,
            id: "protect-shell".to_owned(),
            tool: "policy-check".to_owned(),
            subject: HookSubject::ToolCall,
            phase: HookPhase::Before,
            effect: HookEffect::AllowDeny,
            requirement: HookRequirement::Required,
            timeout_ms: 5_000,
            failure_policy: HookFailurePolicy::FailClosed,
            retry: HookRetryPolicy::Never,
            error_visibility: HookErrorVisibility::ModelAndUser,
            matcher: HookMatcherV1 {
                tool_names: vec!["Bash".to_owned()],
            },
            bindings: vec![HookBindingV1 {
                input: "tool_name".to_owned(),
                field: HookEventField::ToolName,
            }],
            blocking_scope: HookBlockingScope::MatchedEvent,
            fallback: None,
        }
    }

    fn approval_fixture() -> (TempDir, StorePaths, PathBuf) {
        let temp = TempDir::new().expect("tempdir should be created");
        let root = temp.path().join("store");
        store::init_store(root.clone(), false).expect("store should initialize");
        let paths = StorePaths::new(root);
        let package = paths.local_dir.join("plugins/quality");
        fs::create_dir_all(package.join("bin")).expect("tool directory should exist");
        fs::write(
            package.join(crate::plugin::PLUGIN_FILE),
            r#"schema_version = 1
[plugin]
name = "quality"
description = "Quality policy"

[[tool]]
schema_version = 1
id = "detector"
entry = "bin/detect"
runtime = "executable"
platforms = ["macos", "linux"]
argv = ["--cwd", "${input.cwd}"]
cwd = "tool_root"
capabilities = ["filesystem_read"]
availability = "required"

[[tool.inputs]]
name = "cwd"
type = "path"
required = true

[[hook]]
schema_version = 1
id = "protect-shell"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
bindings = [{ input = "cwd", field = "session.cwd" }]
matcher = { tool_names = ["Bash"] }
"#,
        )
        .expect("manifest should write");
        let entry = package.join("bin/detect");
        fs::write(&entry, b"#!/bin/sh\nexit 0\n").expect("entry should write");
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))
            .expect("entry should be executable");
        (temp, paths, package)
    }

    fn assert_fixture_document(document: &str, provider: HookProvider) {
        let fixtures = parse_fixtures(document).expect("fixture document should parse");
        assert!(!fixtures.is_empty());
        let mut ids = BTreeSet::new();
        for fixture in fixtures {
            assert_eq!(fixture.provider, provider);
            assert!(ids.insert(fixture.id.clone()), "duplicate fixture ID");
            let result = evaluate_fixture(&fixture);
            assert_eq!(
                result.compatibility, fixture.expected,
                "fixture {}: {}",
                fixture.id, result.reason
            );
            if fixture.effect.is_pre_action_enforcement()
                && fixture.requirement == HookRequirement::Required
                && result.compatibility == HookCompatibility::GuidanceOnly
            {
                panic!("required enforcement must never degrade to guidance");
            }
        }
    }

    #[test]
    fn codex_adapter_fixtures_match_verified_contract() {
        assert_fixture_document(
            include_str!("../tests/fixtures/hooks/codex-0.147.0.json"),
            HookProvider::Codex,
        );
    }

    #[test]
    fn claude_adapter_fixtures_match_verified_contract() {
        assert_fixture_document(
            include_str!("../tests/fixtures/hooks/claude-2.1.233.json"),
            HookProvider::Claude,
        );
    }

    #[test]
    fn version_drift_downgrades_an_enforcement_claim() {
        let fixture = AdapterFixture {
            id: "version-drift".to_owned(),
            provider: HookProvider::Codex,
            provider_version: "0.148.0".to_owned(),
            subject: HookSubject::ToolCall,
            phase: HookPhase::Before,
            effect: HookEffect::AllowDeny,
            requirement: HookRequirement::Required,
            failure_policy: HookFailurePolicy::FailClosed,
            hooks_enabled: true,
            plugin_hooks_allowed: true,
            actor: HookActor::Root,
            tool_path: ToolPathCoverage::CoveredLocal,
            permission_mode: FixturePermissionMode::Default,
            behavior: FixtureBehavior::Deny,
            expected: HookCompatibility::UnverifiedVersion,
        };
        let result = evaluate_fixture(&fixture);
        assert_eq!(result.compatibility, HookCompatibility::UnverifiedVersion);
        assert!(!result.enforcement_satisfied);
    }

    #[test]
    fn composition_is_order_independent_and_deny_wins() {
        let left = PortableHookResult {
            hook: "team:z".to_owned(),
            output: PortableHookOutput::Allow,
        };
        let right = PortableHookResult {
            hook: "team:a".to_owned(),
            output: PortableHookOutput::Deny {
                reason: "policy".to_owned(),
            },
        };
        let forward = compose_results(HookEffect::AllowDeny, &[left.clone(), right.clone()]);
        let reverse = compose_results(HookEffect::AllowDeny, &[right, left]);
        assert_eq!(forward, reverse);
        assert_eq!(forward.denials, ["policy"]);
    }

    #[test]
    fn divergent_rewrites_fail_closed_as_a_composition_conflict() {
        let results = [
            PortableHookResult {
                hook: "team:a".to_owned(),
                output: PortableHookOutput::RewriteInput {
                    input: serde_json::json!({"command": "one"}),
                },
            },
            PortableHookResult {
                hook: "team:b".to_owned(),
                output: PortableHookOutput::RewriteInput {
                    input: serde_json::json!({"command": "two"}),
                },
            },
        ];
        let outcome = compose_results(HookEffect::RewriteInput, &results);
        assert!(outcome.value.is_none());
        assert!(!outcome.conflicts.is_empty());
    }

    #[test]
    fn descriptor_accepts_required_pre_action_enforcement() {
        validate_descriptor(&valid_descriptor()).expect("descriptor should be valid");
    }

    #[test]
    fn descriptor_requires_an_explicit_optional_omission() {
        let mut descriptor = valid_descriptor();
        descriptor.requirement = HookRequirement::Optional;
        assert!(validate_descriptor(&descriptor).is_err());
        descriptor.fallback = Some(HookFallback::Omit);
        validate_descriptor(&descriptor).expect("optional omission should be explicit");
    }

    #[test]
    fn descriptor_rejects_weakening_required_fallback() {
        let mut descriptor = valid_descriptor();
        descriptor.fallback = Some(HookFallback::Omit);
        assert!(validate_descriptor(&descriptor).is_err());
    }

    #[test]
    fn descriptor_rejects_control_claims_for_observation() {
        let mut descriptor = valid_descriptor();
        descriptor.subject = HookSubject::Session;
        descriptor.phase = HookPhase::End;
        descriptor.effect = HookEffect::Observe;
        descriptor.matcher = HookMatcherV1::default();
        descriptor.bindings.clear();
        assert!(validate_descriptor(&descriptor).is_err());
        descriptor.failure_policy = HookFailurePolicy::Report;
        validate_descriptor(&descriptor).expect("session observation should report failures");
    }

    #[test]
    fn descriptor_rejects_invalid_event_effect_combinations() {
        let mut descriptor = valid_descriptor();
        descriptor.subject = HookSubject::Session;
        descriptor.phase = HookPhase::Before;
        assert!(validate_descriptor(&descriptor).is_err());
    }

    #[test]
    fn hook_approval_is_separate_and_invalidated_by_matcher_drift() {
        let (_temp, paths, package) = approval_fixture();
        let hook_id = "local:quality#hook:protect-shell";
        let tool_id = "local:quality#tool:detector";

        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::ToolUnavailable
        );
        assert!(approve(&paths, hook_id, false).is_err());
        tool::approve(&paths, tool_id, false).expect("tool should be separately approved");
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::PendingApproval
        );
        approve(&paths, hook_id, false).expect("hook should be independently approved");
        assert_eq!(show(&paths, hook_id).unwrap().state, HookTrustState::Ready);

        let manifest_path = package.join(crate::plugin::PLUGIN_FILE);
        let manifest = fs::read_to_string(&manifest_path).unwrap();
        fs::write(
            &manifest_path,
            manifest.replace("[\"Bash\"]", "[\"Write\"]"),
        )
        .unwrap();
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::HashDrift
        );
    }

    #[test]
    fn hook_approval_dry_run_and_revocation_are_scoped() {
        let (_temp, paths, _package) = approval_fixture();
        let hook_id = "local:quality#hook:protect-shell";
        let tool_id = "local:quality#tool:detector";
        tool::approve(&paths, tool_id, false).unwrap();

        let dry_run = approve(&paths, hook_id, true).unwrap();
        assert!(dry_run.dry_run);
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::PendingApproval
        );
        approve(&paths, hook_id, false).unwrap();
        revoke(&paths, hook_id, false).unwrap();
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::PendingApproval
        );
        assert_eq!(tool::show(&paths, tool_id).unwrap().state, ToolState::Ready);
    }

    #[test]
    fn source_advancement_of_the_tool_invalidates_both_approval_boundaries() {
        let (_temp, paths, package) = approval_fixture();
        let hook_id = "local:quality#hook:protect-shell";
        let tool_id = "local:quality#tool:detector";
        tool::approve(&paths, tool_id, false).unwrap();
        approve(&paths, hook_id, false).unwrap();

        fs::write(package.join("bin/detect"), b"#!/bin/sh\nexit 7\n").unwrap();
        assert_eq!(
            tool::show(&paths, tool_id).unwrap().state,
            ToolState::HashDrift
        );
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::ToolUnavailable
        );

        tool::approve(&paths, tool_id, false).unwrap();
        assert_eq!(
            show(&paths, hook_id).unwrap().state,
            HookTrustState::HashDrift
        );
    }

    #[test]
    fn one_approved_hook_projects_to_codex_and_claude_without_data_interpolation() {
        let (_temp, paths, _package) = approval_fixture();
        let hook_id = "local:quality#hook:protect-shell";
        tool::approve(&paths, "local:quality#tool:detector", false).unwrap();
        approve(&paths, hook_id, false).unwrap();
        let mut status = show(&paths, hook_id).unwrap();
        status.hook.descriptor.matcher.tool_names =
            vec!["PowerShell|Bash $(touch nope); \"quoted\" Ω".to_owned()];
        let executable = Path::new("/Program Files/Dalo & Co/dalo.exe");

        let codex = compile_native_projection(
            &paths,
            HookProvider::Codex,
            CODEX_HOOK_BASELINE,
            executable,
            &[status.clone()],
        )
        .unwrap();
        let claude = compile_native_projection(
            &paths,
            HookProvider::Claude,
            CLAUDE_HOOK_BASELINE,
            executable,
            &[status],
        )
        .unwrap();

        let codex_group = &codex.hooks["PreToolUse"][0];
        assert_eq!(
            codex_group["matcher"],
            "^(?:PowerShell\\|Bash \\$\\(touch nope\\); \"quoted\" Ω)$"
        );
        let codex_handler = &codex_group["hooks"][0];
        assert!(
            codex_handler["command"]
                .as_str()
                .unwrap()
                .starts_with("'/Program Files/Dalo & Co/dalo.exe' ")
        );
        assert!(
            codex_handler["commandWindows"]
                .as_str()
                .unwrap()
                .starts_with("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ")
        );
        assert!(
            !codex_handler["commandWindows"]
                .as_str()
                .unwrap()
                .contains("Dalo & Co")
        );
        assert!(
            !codex_handler["command"]
                .as_str()
                .unwrap()
                .contains("touch nope")
        );

        let claude_handler = &claude.hooks["PreToolUse"][0]["hooks"][0];
        assert_eq!(
            claude_handler["command"],
            executable.to_string_lossy().as_ref()
        );
        assert!(claude_handler["args"].is_array());
        assert!(
            !claude_handler["args"]
                .as_array()
                .unwrap()
                .iter()
                .any(|argument| argument
                    .as_str()
                    .is_some_and(|value| value.contains("touch nope")))
        );
    }

    #[test]
    fn unverified_provider_version_blocks_projection() {
        let (_temp, paths, _package) = approval_fixture();
        let error = compile_native_projection(
            &paths,
            HookProvider::Codex,
            "0.148.0",
            Path::new("/usr/bin/dalo"),
            &[],
        )
        .expect_err("version drift must block");
        assert!(error.to_string().contains("unverified"));
    }

    #[test]
    fn windows_launcher_encodes_spaces_quotes_metacharacters_newlines_and_unicode() {
        let cases = [
            r#"C:\Program Files\Dalo\dalo.exe"#,
            r#"C:\quote'and\"double.exe"#,
            r#"C:\meta&|<>^%!\dalo.exe"#,
            "C:\\line\nbreak\\dalo.exe",
            r#"C:\Unicode Ω 雪\dalo.exe"#,
            r#"C:\-EncodedCommand-shaped\dalo.exe"#,
        ];
        for executable in cases {
            let command = powershell_encoded_command(
                Path::new(executable),
                &["--projection".to_owned(), "aa".repeat(32)],
            );
            let encoded = command
                .strip_prefix("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand ")
                .unwrap();
            assert!(encoded.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
            }));
            assert!(!command.contains(executable));
            assert!(!command.contains("cmd.exe"));
        }
    }
}
