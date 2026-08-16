//! Portable hook semantics and executable provider-contract fixtures.
//!
//! This module deliberately does not discover, approve, install, register, or
//! execute hooks. It defines the versioned semantic boundary that later
//! adapter work must satisfy before producing native configuration.

use serde::{Deserialize, Serialize};

/// Codex release against which every fixture claim was verified.
pub const CODEX_HOOK_BASELINE: &str = "0.147.0";
/// Claude Code release against which every fixture claim was verified.
pub const CLAUDE_HOOK_BASELINE: &str = "2.1.233";

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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
            fallback: None,
        }
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
}
