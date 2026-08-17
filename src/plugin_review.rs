//! Aggregated, read-only-first review of portable plugin trust boundaries.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::audit::{self, AuditOptions};
use crate::error::{DaloError, DaloResult};
use crate::hook::HookTrustState;
use crate::plan::{InstallationPlan, PlanCompatibility, TargetComponentState};
use crate::plugin::{MemberRequirement, PluginComponentState, PluginDependencyState, PluginState};
use crate::store::{self, ApprovalRecord, StorePaths};
use crate::tool::ToolState;

/// Current machine-readable aggregated-review schema.
pub const PLUGIN_REVIEW_SCHEMA_VERSION: u32 = 1;

/// One complete, inert review snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct PluginReviewReport {
    /// Explicit review schema version.
    pub schema_version: u32,
    /// Canonical root selected by the user.
    pub root_plugin: String,
    /// Exact selected dependency closure, in canonical order.
    pub plugin_closure: Vec<ReviewPlugin>,
    /// The same typed target and projection facts exposed by `dalo plan`.
    pub installation_plan: InstallationPlan,
    /// Separately scoped trust decisions in deterministic display order.
    pub decisions: Vec<ReviewDecision>,
    /// Hash binding the displayed closure, contracts, and target mappings.
    pub review_token: String,
    /// Aggregated review never executes active components or mutates targets.
    pub read_only: bool,
}

/// Package provenance retained in the reviewed dependency closure.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewPlugin {
    /// Canonical plugin identity.
    pub source_ref: String,
    /// Complete authored package hash.
    pub package_hash: String,
    /// Effective passive closure hash.
    pub closure_hash: String,
    /// Current canonical state.
    pub state: PluginState,
    /// Deterministic blocking explanations.
    pub blocking_reasons: Vec<String>,
}

/// Stable trust-boundary vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecisionKind {
    /// Passive skill content, with a deterministic content audit.
    SkillContent,
    /// Independently activated canonical agent package.
    AgentActivation,
    /// Independently managed instruction recommendation; never approved here.
    InstructionRecommendation,
    /// Exact executable tool contract.
    ToolExecution,
    /// Exact hook event/effect contract.
    HookBinding,
}

/// State of one exact component boundary at snapshot time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecisionState {
    /// Exact existing approval remains valid and is reused.
    Reused,
    /// Exact displayed approval can be selected in this session.
    Pending,
    /// A prior identity approval exists but its hash-bound contract changed.
    Invalidated,
    /// Safety, audit, dependency, or availability prevents approval.
    Blocked,
    /// Target adapters cannot represent the component safely.
    Unsupported,
    /// Component is visible but intentionally not activated by plugin review.
    Inactive,
}

/// One exact, separately approvable (or explicitly non-approvable) boundary.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewDecision {
    /// Stable session identity, also used by interactive prompts.
    pub id: String,
    /// Plugin that declares this component.
    pub plugin: String,
    /// Trust boundary.
    pub kind: ReviewDecisionKind,
    /// Required, optional, or recommended membership when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<MemberRequirement>,
    /// Source-qualified component identity.
    pub component: String,
    /// Exact content or contract hash, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Existing approval scope; never source, author, organization, or wildcard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_scope: Option<String>,
    /// Exact existing approval value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_value: Option<String>,
    /// Whether the exact current record already exists.
    pub approval_reused: bool,
    /// Current review state.
    pub state: ReviewDecisionState,
    /// Stable explanation, including invalidation causes.
    pub diagnostic: String,
    /// Component-specific security and behavior facts.
    pub facts: Vec<ReviewFact>,
    /// Per-target mappings and compatibility.
    pub targets: Vec<ReviewTargetFact>,
}

/// One stable label/value fact shown before a decision.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewFact {
    /// Stable fact label.
    pub label: String,
    /// Human- and machine-readable value.
    pub value: String,
}

/// Target-specific component mapping retained from the installation plan.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewTargetFact {
    /// Logical target ID.
    pub target: String,
    /// Effective target component state.
    pub state: TargetComponentState,
    /// Adapter compatibility result.
    pub compatibility: PlanCompatibility,
    /// Proposed native or portable representation.
    pub mapping: String,
    /// Selected authored fallback, when any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// Result of the explicit final transaction.
#[derive(Debug, Clone, Serialize)]
pub struct PluginReviewCommit {
    /// Root plugin whose snapshot was committed.
    pub root_plugin: String,
    /// Token verified immediately before the write.
    pub review_token: String,
    /// Exact separately scoped values added to the approval ledger.
    pub granted: Vec<ApprovalRecord>,
    /// Displayed pending decisions the user declined or skipped.
    pub declined: Vec<String>,
    /// Whether the approval ledger changed.
    pub changed: bool,
}

/// Build one deterministic review without executing tools, hooks, generators,
/// semantic reviewers, or provider mutations.
pub fn build(store_root: &Path, plugin_ref: &str) -> DaloResult<PluginReviewReport> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let live = crate::resolver::resolve_from_config(&config, approvals.approvals.clone());
    let inventories = live
        .scans
        .iter()
        .filter_map(|scan| scan.inventory.clone())
        .collect::<Vec<_>>();
    let canonical = crate::plugin::normalize_plugin_reference(&config, &inventories, plugin_ref)
        .map_err(|reason| DaloError::StateError { reason })?;
    if !live
        .plugins
        .plugins
        .iter()
        .any(|plugin| plugin.source_ref == canonical)
    {
        return Err(DaloError::InvalidArgument {
            reason: format!(
                "plugin `{canonical}` is not selected; run `dalo plugin select {canonical}` first"
            ),
        });
    }
    let closure = dependency_closure(&live.plugins, &canonical);
    let mut plan = crate::plan::build_installation_plan(store_root, None)?;
    filter_plan(&mut plan, &closure);

    let plugin_closure = plan
        .canonical_plugins
        .plugins
        .iter()
        .map(|plugin| ReviewPlugin {
            source_ref: plugin.source_ref.clone(),
            package_hash: plugin.package_hash.clone(),
            closure_hash: plugin.closure_hash.clone(),
            state: plugin.state,
            blocking_reasons: plugin.blocking_reasons.clone(),
        })
        .collect::<Vec<_>>();
    let mut decisions = Vec::new();
    for plugin in &plan.canonical_plugins.plugins {
        for member in &plugin.members {
            decisions.push(member_decision(
                &paths,
                &plan,
                &inventories,
                &approvals.approvals,
                &plugin.source_ref,
                member,
            ));
        }
    }
    for status in &plan.tools {
        decisions.push(tool_decision(&plan, &approvals.approvals, status));
    }
    for status in &plan.hooks {
        decisions.push(hook_decision(&plan, &approvals.approvals, status));
    }
    decisions.sort_by(|left, right| {
        left.plugin
            .cmp(&right.plugin)
            .then(left.kind.cmp(&right.kind))
            .then(left.component.cmp(&right.component))
    });
    let review_token = review_token(&canonical, &plugin_closure, &decisions, &plan)?;
    Ok(PluginReviewReport {
        schema_version: PLUGIN_REVIEW_SCHEMA_VERSION,
        root_plugin: canonical,
        plugin_closure,
        installation_plan: plan,
        decisions,
        review_token,
        read_only: true,
    })
}

/// Commit an explicitly selected subset after rebuilding and matching the exact
/// review token. All records are appended with one atomic approval-ledger write.
pub fn commit(
    store_root: &Path,
    plugin_ref: &str,
    expected_token: &str,
    selected: &BTreeSet<String>,
) -> DaloResult<PluginReviewCommit> {
    let paths = StorePaths::new(store_root.to_path_buf());
    let current = build(store_root, plugin_ref)?;
    if current.review_token != expected_token {
        return Err(DaloError::StateError {
            reason: "plugin review changed after display; no approvals were granted—review again"
                .to_owned(),
        });
    }
    let pending = current
        .decisions
        .iter()
        .filter(|decision| {
            matches!(
                decision.state,
                ReviewDecisionState::Pending | ReviewDecisionState::Invalidated
            ) && decision.approval_value.is_some()
        })
        .map(|decision| (decision.id.as_str(), decision))
        .collect::<BTreeMap<_, _>>();
    for id in selected {
        if !pending.contains_key(id.as_str()) {
            return Err(DaloError::InvalidArgument {
                reason: format!("decision `{id}` was not shown as approvable; nothing was written"),
            });
        }
    }

    // Validate every selected boundary before preparing any inert tool bytes.
    for id in selected {
        let decision = pending[id.as_str()];
        if decision.kind == ReviewDecisionKind::SkillContent {
            let report = audit::audit_target(
                &paths,
                &decision.component,
                &AuditOptions {
                    persist: false,
                    ..AuditOptions::default()
                },
            )?;
            if report.is_blocking()
                || decision.content_hash.as_deref() != Some(report.content_hash.as_str())
            {
                return Err(DaloError::AuditBlocked {
                    reason: format!(
                        "skill `{}` no longer matches its displayed non-blocking audit",
                        decision.component
                    ),
                });
            }
        }
        if decision.kind == ReviewDecisionKind::HookBinding {
            let hook = crate::hook::show(&paths, &decision.component)?;
            let tool_id = format!("tool:{}", hook.hook.tool_source_ref);
            if hook.tool_state != ToolState::Ready && !selected.contains(&tool_id) {
                return Err(DaloError::StateError {
                    reason: format!(
                        "hook `{}` requires its separately reviewed tool `{}`",
                        decision.component, hook.hook.tool_source_ref
                    ),
                });
            }
        }
    }
    for id in selected {
        let decision = pending[id.as_str()];
        if decision.kind == ReviewDecisionKind::ToolExecution {
            crate::tool::prepare_approval(&paths, &decision.component)?;
        }
    }

    let mut approvals = store::read_approvals(&paths)?;
    let mut granted = Vec::new();
    for id in selected {
        let decision = pending[id.as_str()];
        let record = ApprovalRecord {
            scope: decision
                .approval_scope
                .clone()
                .expect("pending approval scope"),
            value: decision
                .approval_value
                .clone()
                .expect("pending approval value"),
        };
        if !approvals.approvals.contains(&record) {
            approvals.approvals.push(record.clone());
            granted.push(record);
        }
    }
    approvals.approvals.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then(left.value.cmp(&right.value))
    });
    if !granted.is_empty() {
        store::write_approvals(&paths, &approvals)?;
    }
    let declined = pending
        .keys()
        .filter(|id| !selected.contains(**id))
        .map(|id| (*id).to_owned())
        .collect();
    Ok(PluginReviewCommit {
        root_plugin: current.root_plugin,
        review_token: expected_token.to_owned(),
        changed: !granted.is_empty(),
        granted,
        declined,
    })
}

fn dependency_closure(plugins: &crate::plugin::PluginResolution, root: &str) -> BTreeSet<String> {
    let mut closure = BTreeSet::new();
    let mut queue = VecDeque::from([root.to_owned()]);
    while let Some(identity) = queue.pop_front() {
        if !closure.insert(identity.clone()) {
            continue;
        }
        if let Some(plugin) = plugins
            .plugins
            .iter()
            .find(|plugin| plugin.source_ref == identity)
        {
            for dependency in &plugin.dependencies {
                if dependency.state == PluginDependencyState::Selected
                    && let Some(resolved) = &dependency.resolved_ref
                {
                    queue.push_back(resolved.clone());
                }
            }
        }
    }
    closure
}

fn filter_plan(plan: &mut InstallationPlan, closure: &BTreeSet<String>) {
    plan.canonical_plugins
        .plugins
        .retain(|plugin| closure.contains(&plugin.source_ref));
    plan.canonical_plugins.diagnostics.retain(|diagnostic| {
        closure.iter().any(|plugin| {
            diagnostic.subject == *plugin || diagnostic.subject.starts_with(&format!("{plugin}#"))
        })
    });
    plan.tools.retain(|status| {
        closure.iter().any(|plugin| {
            status
                .tool
                .source_ref
                .starts_with(&format!("{plugin}#tool:"))
        })
    });
    plan.hooks.retain(|status| {
        closure.iter().any(|plugin| {
            status
                .hook
                .source_ref
                .starts_with(&format!("{plugin}#hook:"))
        })
    });
    plan.native_plugins
        .retain(|report| closure.contains(&report.plugin));
    for destination in &mut plan.destinations {
        for target in &mut destination.logical_targets {
            target
                .plugins
                .retain(|plugin| closure.contains(&plugin.source_ref));
        }
    }
}

fn member_decision(
    paths: &StorePaths,
    plan: &InstallationPlan,
    inventories: &[crate::inventory::SourceInventory],
    approvals: &[ApprovalRecord],
    plugin: &str,
    member: &crate::plugin::ResolvedPluginMember,
) -> ReviewDecision {
    let component = member
        .resolved_ref
        .clone()
        .unwrap_or_else(|| format!("{plugin}#{}", member.reference));
    let (kind, scope) = if member.reference.starts_with("skill:") {
        (ReviewDecisionKind::SkillContent, Some("skill"))
    } else if member.reference.starts_with("agent:") {
        (ReviewDecisionKind::AgentActivation, Some("agent"))
    } else {
        (ReviewDecisionKind::InstructionRecommendation, None)
    };
    let approval_value = scope.and(member.resolved_ref.clone());
    let approval_reused = scope.is_some_and(|scope| {
        approval_value.as_ref().is_some_and(|value| {
            approvals
                .iter()
                .any(|record| record.scope == scope && record.value == *value)
        })
    });
    let mut content_hash = None;
    let mut facts = Vec::new();
    let mut diagnostic = format!("canonical component is {:?}", member.state).to_lowercase();
    let mut state = match member.state {
        PluginComponentState::Active => ReviewDecisionState::Reused,
        PluginComponentState::PendingApproval => ReviewDecisionState::Pending,
        PluginComponentState::Inactive | PluginComponentState::Available => {
            ReviewDecisionState::Inactive
        }
        PluginComponentState::Shadowed
        | PluginComponentState::Blocked
        | PluginComponentState::Missing
        | PluginComponentState::Ambiguous => ReviewDecisionState::Blocked,
    };
    if kind == ReviewDecisionKind::SkillContent && member.resolved_ref.is_some() {
        match audit::audit_target(
            paths,
            &component,
            &AuditOptions {
                persist: false,
                ..AuditOptions::default()
            },
        ) {
            Ok(audit) => {
                content_hash = Some(audit.content_hash.clone());
                facts.push(fact(
                    "audit_status",
                    format!("{:?}", audit.status).to_lowercase(),
                ));
                facts.push(fact(
                    "audit_coverage",
                    format!("{:?}", audit.coverage).to_lowercase(),
                ));
                facts.push(fact("audit_findings", audit.static_findings.len()));
                if audit.is_blocking() {
                    state = ReviewDecisionState::Blocked;
                    diagnostic =
                        "deterministic content audit has unaccepted blocking findings".to_owned();
                }
            }
            Err(error) => {
                state = ReviewDecisionState::Blocked;
                diagnostic = format!("content audit failed: {error}");
            }
        }
    } else if kind == ReviewDecisionKind::AgentActivation {
        if let Some(agent) = inventories
            .iter()
            .flat_map(|inventory| &inventory.agents)
            .find(|agent| agent.source_ref == component)
        {
            content_hash = Some(agent.content_hash.clone());
            facts.push(fact("description", &agent.description));
            facts.push(fact("required_skills", agent.skills.join(",")));
            facts.push(fact(
                "targets",
                agent
                    .targets
                    .clone()
                    .unwrap_or_else(|| vec!["all".to_owned()])
                    .join(","),
            ));
            facts.push(fact("support_files", agent.has_support_files));
        }
    } else {
        facts.push(fact(
            "activation",
            "recommendation only; plugin review never enables instruction packs",
        ));
        state = ReviewDecisionState::Inactive;
    }
    let targets = target_facts(plan, plugin, &member.reference, &component);
    ReviewDecision {
        id: format!("{}:{component}", kind_id(kind)),
        plugin: plugin.to_owned(),
        kind,
        requirement: Some(member.requirement),
        component,
        content_hash,
        approval_scope: scope.map(str::to_owned),
        approval_value,
        approval_reused,
        state,
        diagnostic,
        facts,
        targets,
    }
}

fn tool_decision(
    plan: &InstallationPlan,
    approvals: &[ApprovalRecord],
    status: &crate::tool::ToolStatusReport,
) -> ReviewDecision {
    let reused = approvals.iter().any(|record| {
        record.scope == crate::tool::APPROVAL_SCOPE && record.value == status.approval_value
    });
    let state = match status.state {
        ToolState::Ready => ReviewDecisionState::Reused,
        ToolState::PendingApproval | ToolState::Revoked => ReviewDecisionState::Pending,
        ToolState::HashDrift => ReviewDecisionState::Invalidated,
        ToolState::PlatformMismatch
        | ToolState::RuntimeMissing
        | ToolState::ApprovedNotStaged
        | ToolState::AuditFailure => ReviewDecisionState::Blocked,
    };
    let plugin = status
        .tool
        .source_ref
        .split_once("#tool:")
        .map_or("", |(plugin, _)| plugin)
        .to_owned();
    ReviewDecision {
        id: format!("tool:{}", status.tool.source_ref),
        plugin: plugin.clone(),
        kind: ReviewDecisionKind::ToolExecution,
        requirement: None,
        component: status.tool.source_ref.clone(),
        content_hash: Some(status.tool.contract_hash.clone()),
        approval_scope: Some(crate::tool::APPROVAL_SCOPE.to_owned()),
        approval_value: Some(status.approval_value.clone()),
        approval_reused: reused,
        state,
        diagnostic: status.diagnostic.clone(),
        facts: vec![
            fact("entry", &status.tool.entry),
            fact(
                "runtime",
                format!("{:?}", status.tool.runtime).to_lowercase(),
            ),
            fact("capabilities", format!("{:?}", status.tool.capabilities)),
            fact("environment", status.tool.env.join(",")),
            fact("closure_files", status.tool.files.len()),
        ],
        targets: target_facts(
            plan,
            &plugin,
            &status.tool.source_ref,
            &status.tool.source_ref,
        ),
    }
}

fn hook_decision(
    plan: &InstallationPlan,
    approvals: &[ApprovalRecord],
    status: &crate::hook::HookStatusReport,
) -> ReviewDecision {
    let reused = approvals.iter().any(|record| {
        record.scope == crate::hook::APPROVAL_SCOPE && record.value == status.approval_value
    });
    let state = match status.state {
        HookTrustState::Ready => ReviewDecisionState::Reused,
        HookTrustState::PendingApproval => ReviewDecisionState::Pending,
        HookTrustState::HashDrift => ReviewDecisionState::Invalidated,
        // The referenced tool may be approved in the same final transaction.
        HookTrustState::ToolUnavailable if !reused => ReviewDecisionState::Pending,
        HookTrustState::ToolUnavailable => ReviewDecisionState::Blocked,
    };
    let plugin = status
        .hook
        .source_ref
        .split_once("#hook:")
        .map_or("", |(plugin, _)| plugin)
        .to_owned();
    let descriptor = &status.hook.descriptor;
    ReviewDecision {
        id: format!("hook:{}", status.hook.source_ref),
        plugin: plugin.clone(),
        kind: ReviewDecisionKind::HookBinding,
        requirement: Some(match descriptor.requirement {
            crate::hook::HookRequirement::Required => MemberRequirement::Required,
            crate::hook::HookRequirement::Optional => MemberRequirement::Optional,
        }),
        component: status.hook.source_ref.clone(),
        content_hash: Some(status.hook.contract_hash.clone()),
        approval_scope: Some(crate::hook::APPROVAL_SCOPE.to_owned()),
        approval_value: Some(status.approval_value.clone()),
        approval_reused: reused,
        state,
        diagnostic: status.diagnostic.clone(),
        facts: vec![
            fact("tool", &status.hook.tool_source_ref),
            fact(
                "event",
                format!("{:?}/{:?}", descriptor.subject, descriptor.phase),
            ),
            fact("effect", format!("{:?}", descriptor.effect).to_lowercase()),
            fact("matcher", format!("{:?}", descriptor.matcher.tool_names)),
            fact("timeout_ms", descriptor.timeout_ms),
            fact(
                "failure_policy",
                format!("{:?}", descriptor.failure_policy).to_lowercase(),
            ),
            fact("bindings", descriptor.bindings.len()),
        ],
        targets: target_facts(
            plan,
            &plugin,
            &status.hook.source_ref,
            &status.hook.source_ref,
        ),
    }
}

fn target_facts(
    plan: &InstallationPlan,
    plugin: &str,
    authored_reference: &str,
    component: &str,
) -> Vec<ReviewTargetFact> {
    let mut facts = Vec::new();
    for destination in &plan.destinations {
        for target in &destination.logical_targets {
            if let Some(plugin_plan) = target
                .plugins
                .iter()
                .find(|candidate| candidate.source_ref == plugin)
            {
                for candidate in &plugin_plan.components {
                    if candidate.reference == authored_reference || candidate.reference == component
                    {
                        facts.push(ReviewTargetFact {
                            target: target.id.clone(),
                            state: candidate.state,
                            compatibility: candidate.compatibility,
                            mapping: candidate.proposed_artifact.clone(),
                            fallback: candidate.selected_fallback.clone(),
                        });
                    }
                }
            }
        }
    }
    facts.sort_by(|left, right| left.target.cmp(&right.target));
    facts.dedup_by(|left, right| left.target == right.target);
    facts
}

fn fact(label: &str, value: impl ToString) -> ReviewFact {
    ReviewFact {
        label: label.to_owned(),
        value: value.to_string(),
    }
}

const fn kind_id(kind: ReviewDecisionKind) -> &'static str {
    match kind {
        ReviewDecisionKind::SkillContent => "skill",
        ReviewDecisionKind::AgentActivation => "agent",
        ReviewDecisionKind::InstructionRecommendation => "instruction",
        ReviewDecisionKind::ToolExecution => "tool",
        ReviewDecisionKind::HookBinding => "hook",
    }
}

fn review_token(
    root: &str,
    plugins: &[ReviewPlugin],
    decisions: &[ReviewDecision],
    plan: &InstallationPlan,
) -> DaloResult<String> {
    let decision_facts = decisions
        .iter()
        .map(|decision| {
            let targets = decision
                .targets
                .iter()
                .map(|target| {
                    let mapping = if decision.kind == ReviewDecisionKind::ToolExecution {
                        decision
                            .content_hash
                            .as_ref()
                            .map(|hash| format!("immutable tool root sha256:{hash}"))
                    } else {
                        Some(target.mapping.clone())
                    };
                    serde_json::json!({
                        "target": target.target,
                        "state": target.state,
                        "compatibility": target.compatibility,
                        "mapping": mapping,
                        "fallback": target.fallback,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": decision.id,
                "plugin": decision.plugin,
                "kind": decision.kind,
                "requirement": decision.requirement,
                "component": decision.component,
                "content_hash": decision.content_hash,
                "approval_scope": decision.approval_scope,
                "approval_value": decision.approval_value,
                "approval_reused": decision.approval_reused,
                "state": decision.state,
                "facts": decision.facts,
                "targets": targets,
            })
        })
        .collect::<Vec<_>>();
    let target_facts = plan
        .destinations
        .iter()
        .map(|destination| {
            let logical_targets = destination
                .logical_targets
                .iter()
                .map(|target| {
                    let plugins = target
                        .plugins
                        .iter()
                        .map(|plugin| {
                            let components = plugin
                                .components
                                .iter()
                                .map(|component| {
                                    serde_json::json!({
                                        "reference": component.reference,
                                        "requirement": component.requirement,
                                        "canonical_state": component.canonical_state,
                                        "state": component.state,
                                        "compatibility": component.compatibility,
                                        "authored_fallback": component.authored_fallback,
                                        "selected_fallback": component.selected_fallback,
                                        "blocker": component.blocker,
                                    })
                                })
                                .collect::<Vec<_>>();
                            serde_json::json!({
                                "source_ref": plugin.source_ref,
                                "state": plugin.state,
                                "compatibility": plugin.compatibility,
                                "dependencies": plugin.dependencies,
                                "components": components,
                                "blockers": plugin.blockers,
                            })
                        })
                        .collect::<Vec<_>>();
                    serde_json::json!({
                        "id": target.id,
                        "verification_baseline": target.verification_baseline,
                        "plugins": plugins,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "path": destination.path,
                "logical_targets": logical_targets,
            })
        })
        .collect::<Vec<_>>();
    let native_facts = plan
        .native_plugins
        .iter()
        .map(|projection| {
            let components = projection
                .components
                .iter()
                .map(|component| {
                    serde_json::json!({
                        "identity": component.identity,
                        "kind": component.kind,
                        "native_path": component.native_path,
                        "content_hash": component.content_hash,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "target": projection.target,
                "plugin": projection.plugin,
                "native_name": projection.native_name,
                "path": projection.path,
                "adapter_baseline": projection.adapter_baseline,
                "state": projection.state,
                "components": components,
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&(root, plugins, decision_facts, target_facts, native_facts))?;
    let digest = Sha256::digest(bytes);
    Ok(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}
