//! Runtime dispatcher for hash-addressed, independently approved hook projections.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::tempfile;

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};

use crate::error::{DaloError, DaloResult};
use crate::hook::{
    HookDescriptorV1, HookEffect, HookEventField, HookFailurePolicy, HookProvider,
    PortableHookOutput, PortableHookResult, compose_results,
};
use crate::plugin::{ToolRecord, ToolRuntime};
use crate::store::StorePaths;
use crate::tool;

const MAX_HANDLER_OUTPUT: u64 = 4 * 1024 * 1024;
const HANDLER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Exact hidden-dispatch invocation selected by a native sidecar.
#[derive(Debug, Clone)]
pub struct DispatchRequest<'a> {
    /// Native provider that emitted the event.
    pub provider: HookProvider,
    /// Content-addressed projection manifest hash.
    pub projection: &'a str,
    /// Provider-native event name.
    pub event: &'a str,
    /// Exact dispatcher group selected by the native matcher.
    pub group: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatcherManifest {
    schema_version: u32,
    provider: HookProvider,
    provider_version: String,
    hooks: Vec<DispatcherHook>,
    groups: Vec<DispatcherGroup>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatcherHook {
    identity: String,
    contract_hash: String,
    tool: String,
    tool_contract_hash: String,
    tool_root: PathBuf,
    tool_contract: ToolRecord,
    descriptor: HookDescriptorV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatcherGroup {
    id: String,
    event: String,
    matcher: Option<String>,
    hooks: Vec<String>,
}

/// Validate a projection, execute only its immutable tools, and translate output.
pub fn dispatch(
    paths: &StorePaths,
    request: &DispatchRequest<'_>,
    native_input: &[u8],
) -> DaloResult<Value> {
    validate_hash(request.projection)?;
    let manifest_path = paths
        .hooks_dir
        .join("projections")
        .join(format!("{}.json", request.projection));
    let bytes = fs::read(&manifest_path)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    let canonical = serde_json::to_vec(&value)?;
    if hash_bytes(&canonical) != request.projection {
        return Err(DaloError::StateError {
            reason: "dispatcher projection bytes do not match their content address".to_owned(),
        });
    }
    let manifest: DispatcherManifest = serde_json::from_value(value)?;
    if manifest.schema_version != 1
        || manifest.provider != request.provider
        || manifest.provider_version != request.provider.baseline()
    {
        return Err(DaloError::StateError {
            reason: "dispatcher projection provider or verified version mismatch".to_owned(),
        });
    }
    let group = manifest
        .groups
        .iter()
        .find(|group| group.id == request.group && group.event == request.event)
        .ok_or_else(|| DaloError::StateError {
            reason: "dispatcher group does not match the native event".to_owned(),
        })?;
    let _matcher_is_native_only = &group.matcher;
    let selected = group.hooks.iter().cloned().collect::<BTreeSet<_>>();
    if selected.len() != group.hooks.len() {
        return Err(DaloError::StateError {
            reason: "dispatcher group contains duplicate hook identities".to_owned(),
        });
    }
    let input: Value =
        serde_json::from_slice(native_input).map_err(|error| DaloError::StateError {
            reason: format!("native hook input is malformed JSON: {error}"),
        })?;
    let mut outputs: BTreeMap<HookEffect, Vec<PortableHookResult>> = BTreeMap::new();
    for hook in manifest
        .hooks
        .iter()
        .filter(|hook| selected.contains(&hook.identity))
    {
        if !crate::hook::provider_supports_descriptor(request.provider, &hook.descriptor) {
            return Err(DaloError::StateError {
                reason: format!(
                    "hook `{}` is not supported by the verified {:?} adapter",
                    hook.identity, request.provider
                ),
            });
        }
        verify_hook(paths, hook)?;
        let output = match invoke_hook(hook, &input, native_input) {
            Ok(output) => output,
            Err(error) => failure_output(hook, &error.to_string())?,
        };
        outputs
            .entry(hook.descriptor.effect)
            .or_default()
            .push(PortableHookResult {
                hook: hook.identity.clone(),
                output,
            });
    }
    if outputs.values().map(Vec::len).sum::<usize>() != selected.len() {
        return Err(DaloError::StateError {
            reason: "dispatcher group references a missing hook contract".to_owned(),
        });
    }
    // SessionEnd stdout is ignored by the pinned native adapters. Returning a
    // failure here makes a user-only diagnostic observable through the
    // dispatch command's non-zero exit status instead of silently reporting
    // success with an envelope the provider will discard.
    if request.event == "SessionEnd"
        && let Some(reason) = session_end_failure(&outputs)
    {
        return Err(DaloError::StateError { reason });
    }
    Ok(render_native_output(
        request.provider,
        request.event,
        &outputs,
    ))
}

fn verify_hook(paths: &StorePaths, hook: &DispatcherHook) -> DaloResult<()> {
    if hook.tool != hook.tool_contract.source_ref
        || hook.tool_contract_hash != hook.tool_contract.contract_hash
    {
        return Err(DaloError::StateError {
            reason: format!("hook `{}` tool contract identity mismatch", hook.identity),
        });
    }
    let expected_root = paths
        .tools_dir
        .join("sha256")
        .join(&hook.tool_contract_hash);
    if hook.tool_root != expected_root
        || !tool::verify_staged_contract(&hook.tool_contract, &expected_root)
    {
        return Err(DaloError::StateError {
            reason: format!(
                "hook `{}` immutable tool closure failed audit",
                hook.identity
            ),
        });
    }
    let expected_hook_hash = crate::hook::contract_hash(
        &hook.identity,
        &hook.descriptor,
        &hook.tool,
        &hook.tool_contract_hash,
    );
    if expected_hook_hash != hook.contract_hash {
        return Err(DaloError::StateError {
            reason: format!("hook `{}` contract hash mismatch", hook.identity),
        });
    }
    // A stored projection outlives its approvals. Recheck the exact grants at
    // execution time so revocation also disables an already installed sidecar.
    let approvals = crate::store::read_approvals(paths)?;
    for (scope, identity, hash) in [
        (
            crate::hook::APPROVAL_SCOPE,
            &hook.identity,
            &hook.contract_hash,
        ),
        (tool::APPROVAL_SCOPE, &hook.tool, &hook.tool_contract_hash),
    ] {
        let value = format!("{identity}@sha256:{hash}");
        if !approvals
            .approvals
            .iter()
            .any(|record| record.scope == scope && record.value == value)
        {
            return Err(DaloError::StateError {
                reason: format!(
                    "hook `{}` requires current exact {scope} approval for `{identity}`",
                    hook.identity
                ),
            });
        }
    }
    Ok(())
}

fn invoke_hook(
    hook: &DispatcherHook,
    input: &Value,
    native_input: &[u8],
) -> DaloResult<PortableHookOutput> {
    let declared_inputs = hook
        .tool_contract
        .inputs
        .iter()
        .map(|input| (input.name.as_str(), input.required))
        .collect::<BTreeMap<_, _>>();
    let mut values = BTreeMap::new();
    for binding in &hook.descriptor.bindings {
        match extract_field(binding.field, input)? {
            Some(value) => {
                values.insert(binding.input.clone(), value);
            }
            None if declared_inputs.get(binding.input.as_str()) == Some(&false) => {}
            None => {
                return Err(DaloError::StateError {
                    reason: format!(
                        "required event field `{}` is absent",
                        binding.field.as_str()
                    ),
                });
            }
        }
    }
    let mut argv = tool::build_argv(&hook.tool_contract, &hook.tool_root, &values)?;
    if argv.is_empty() {
        return Err(DaloError::StateError {
            reason: "hook tool produced an empty argv".to_owned(),
        });
    }
    if hook.tool_contract.runtime != ToolRuntime::Executable {
        argv[0] = resolve_executable(&argv[0])?;
    }
    // `main` restores the Unix SIGPIPE default so dalo remains pipeline-friendly
    // when its own stdout closes. Prewrite the untrusted event payload to an
    // anonymous, seekable file instead of a child-stdin pipe: this both keeps
    // early handler exits from delivering SIGPIPE and removes the mutual pipe
    // dependency between a chatty handler and a large native input.
    let mut stdin_file = tempfile()?;
    stdin_file.write_all(native_input)?;
    stdin_file.flush()?;
    stdin_file.seek(SeekFrom::Start(0))?;

    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&hook.tool_root)
        .stdin(Stdio::from(stdin_file))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear();
    for name in &hook.tool_contract.env {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));
    let deadline = Instant::now() + Duration::from_millis(u64::from(hook.descriptor.timeout_ms));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_handler_process(&mut child);
                return Err(error.into());
            }
        }
        if Instant::now() >= deadline {
            terminate_handler_process(&mut child);
            return Err(DaloError::StateError {
                reason: "hook handler timed out".to_owned(),
            });
        }
        thread::sleep(Duration::from_millis(5));
    };
    terminate_handler_process_group(&mut child)?;
    let stdout = stdout_reader.join().map_err(|_| DaloError::StateError {
        reason: "hook stdout reader failed".to_owned(),
    })??;
    let stderr = stderr_reader.join().map_err(|_| DaloError::StateError {
        reason: "hook stderr reader failed".to_owned(),
    })??;
    if !status.success() {
        return Err(DaloError::StateError {
            reason: format!(
                "hook handler exited with {status}: {}",
                String::from_utf8_lossy(&stderr)
            ),
        });
    }
    if stdout.is_empty() && hook.descriptor.effect == HookEffect::Observe {
        return Ok(PortableHookOutput::Observe);
    }
    let output: PortableHookOutput =
        serde_json::from_slice(&stdout).map_err(|error| DaloError::StateError {
            reason: format!("hook handler returned malformed output: {error}"),
        })?;
    validate_output(hook.descriptor.effect, &output)?;
    Ok(output)
}

fn validate_output(effect: HookEffect, output: &PortableHookOutput) -> DaloResult<()> {
    let compatible = match output {
        // A handler may explicitly abstain from an effect. `observe` is an
        // effect of its own and cannot silently stand in for another effect.
        PortableHookOutput::Observe => effect == HookEffect::Observe,
        PortableHookOutput::Abstain => true,
        PortableHookOutput::AddContext { context } => {
            effect == HookEffect::AddContext
                && context.len() <= 16 * 1024
                && !context.contains('\0')
        }
        PortableHookOutput::Allow => effect == HookEffect::AllowDeny,
        PortableHookOutput::Deny { reason } => {
            effect == HookEffect::AllowDeny && reason.len() <= 16 * 1024 && !reason.contains('\0')
        }
        PortableHookOutput::RewriteInput { input } => {
            effect == HookEffect::RewriteInput
                && input.is_object()
                && serde_json::to_vec(input).is_ok_and(|bytes| bytes.len() <= 1024 * 1024)
        }
        PortableHookOutput::ReplaceOutput { output } => {
            effect == HookEffect::ReplaceOutput
                && serde_json::to_vec(output)
                    .is_ok_and(|bytes| bytes.len() <= MAX_HANDLER_OUTPUT as usize)
        }
        PortableHookOutput::ContinueWorkflow { reason } => {
            effect == HookEffect::ContinueWorkflow
                && reason.len() <= 16 * 1024
                && !reason.contains('\0')
        }
        // This is an internal result and is never valid on handler stdout.
        PortableHookOutput::Failure { .. } => false,
        PortableHookOutput::FailureDeny { .. } | PortableHookOutput::FailureContinue { .. } => {
            false
        }
    };
    if compatible {
        Ok(())
    } else {
        Err(DaloError::StateError {
            reason: format!("hook output is incompatible with declared effect {effect}"),
        })
    }
}

/// What the kernel reported about the handler process group when it was
/// signalled or probed with `kill(-pgid, …)`.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessGroupProbe {
    /// At least one member is still there.
    Present,
    /// Nothing is left to signal.
    Gone,
}

/// Interpret a `kill(-pgid, …)` result.
///
/// macOS reports an empty process group as `EPERM` after its leader has been
/// reaped. Linux uses `EPERM` for a live group whose members cannot be
/// signalled, so only macOS can treat it as gone.
#[cfg(unix)]
fn interpret_process_group_signal(
    result: Result<(), rustix::io::Errno>,
) -> Result<ProcessGroupProbe, rustix::io::Errno> {
    match result {
        Ok(()) => Ok(ProcessGroupProbe::Present),
        Err(rustix::io::Errno::SRCH) => Ok(ProcessGroupProbe::Gone),
        #[cfg(target_os = "macos")]
        Err(rustix::io::Errno::PERM) => Ok(ProcessGroupProbe::Gone),
        Err(error) => Err(error),
    }
}

fn terminate_handler_process_group(child: &mut Child) -> DaloResult<()> {
    #[cfg(unix)]
    {
        let process_group = Pid::from_child(child);
        if let Err(error) =
            interpret_process_group_signal(kill_process_group(process_group, Signal::KILL))
        {
            return Err(DaloError::StateError {
                reason: format!("failed to terminate hook-handler process group: {error}"),
            });
        }
        let _ = child.wait();
        let deadline = Instant::now() + HANDLER_SHUTDOWN_TIMEOUT;
        loop {
            match interpret_process_group_signal(test_kill_process_group(process_group)) {
                Ok(ProcessGroupProbe::Gone) => return Ok(()),
                Ok(ProcessGroupProbe::Present) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(ProcessGroupProbe::Present) => {
                    return Err(DaloError::StateError {
                        reason: "hook-handler process group survived termination; output was not audited"
                            .to_owned(),
                    });
                }
                Err(error) => {
                    return Err(DaloError::StateError {
                        reason: format!(
                            "failed to verify hook-handler process-group termination: {error}"
                        ),
                    });
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        child.kill()?;
        child.wait()?;
        Ok(())
    }
}

fn terminate_handler_process(child: &mut Child) {
    if terminate_handler_process_group(child).is_ok() {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take(MAX_HANDLER_OUTPUT + 1)
        .read_to_end(&mut output)?;
    if output.len() as u64 > MAX_HANDLER_OUTPUT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "hook output exceeds 4 MiB",
        ));
    }
    Ok(output)
}

fn failure_output(hook: &DispatcherHook, reason: &str) -> DaloResult<PortableHookOutput> {
    let bounded = if reason.contains("timed out") {
        "hook handler timed out"
    } else if reason.contains("malformed output") {
        "hook handler returned malformed output"
    } else if reason.contains("required event field") {
        "required hook event field is unavailable"
    } else if reason.contains("exited with") {
        "hook handler exited unsuccessfully"
    } else if reason.contains("incompatible with declared effect") {
        "hook handler returned output incompatible with declared effect"
    } else {
        "hook handler failed validation or execution"
    }
    .to_owned();
    match (hook.descriptor.effect, hook.descriptor.failure_policy) {
        (HookEffect::AllowDeny | HookEffect::RewriteInput, HookFailurePolicy::FailClosed) => {
            Ok(PortableHookOutput::FailureDeny {
                reason: bounded,
                visibility: hook.descriptor.error_visibility,
            })
        }
        (HookEffect::ContinueWorkflow, HookFailurePolicy::FailClosed) => {
            Ok(PortableHookOutput::FailureContinue {
                reason: bounded,
                visibility: hook.descriptor.error_visibility,
            })
        }
        (_, HookFailurePolicy::FailOpen | HookFailurePolicy::Report) => {
            Ok(PortableHookOutput::Failure {
                reason: bounded,
                visibility: hook.descriptor.error_visibility,
            })
        }
        _ => Err(DaloError::StateError { reason: bounded }),
    }
}

fn render_native_output(
    provider: HookProvider,
    event: &str,
    outputs: &BTreeMap<HookEffect, Vec<PortableHookResult>>,
) -> Value {
    let mut context = Vec::new();
    let mut denials = Vec::new();
    let mut rewrite = None;
    let mut replacement = None;
    let mut continuation = false;
    let mut conflicts = Vec::new();
    let mut user_messages = Vec::new();
    for (effect, results) in outputs {
        user_messages.extend(results.iter().filter_map(|result| match &result.output {
            PortableHookOutput::Failure { reason, .. }
            | PortableHookOutput::FailureDeny { reason, .. }
            | PortableHookOutput::FailureContinue { reason, .. } => {
                Some(format!("hook `{}`: {reason}", result.hook))
            }
            _ => None,
        }));
        let outcome = compose_results(*effect, results);
        context.extend(outcome.context);
        denials.extend(outcome.denials);
        conflicts.extend(outcome.conflicts);
        match effect {
            HookEffect::RewriteInput => rewrite = outcome.value,
            HookEffect::ReplaceOutput => replacement = outcome.value,
            HookEffect::ContinueWorkflow => continuation |= outcome.continue_workflow,
            _ => {}
        }
    }
    denials.extend(conflicts);
    let reason = denials.join("\n");
    if event == "PreToolUse" {
        if !reason.is_empty() {
            let mut specific = json!({
                "hookEventName": event,
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            });
            if !context.is_empty() {
                specific["additionalContext"] = Value::String(context.join("\n"));
            }
            return with_user_messages(json!({"hookSpecificOutput": specific}), user_messages);
        }
        if let Some(input) = rewrite {
            let mut specific = json!({
                "hookEventName": event,
                "updatedInput": input,
            });
            // Codex requires this pairing for rewrites; Claude accepts a
            // rewrite alone, preserving its ordinary permission prompt.
            if provider == HookProvider::Codex {
                specific["permissionDecision"] = Value::String("allow".to_owned());
            }
            if !context.is_empty() {
                specific["additionalContext"] = Value::String(context.join("\n"));
            }
            return with_user_messages(json!({"hookSpecificOutput": specific}), user_messages);
        }
    }
    if event == "UserPromptSubmit" && !reason.is_empty() {
        let mut output = json!({"decision": "block", "reason": reason});
        if !context.is_empty() {
            output["hookSpecificOutput"] = json!({
                "hookEventName": event,
                "additionalContext": context.join("\n"),
            });
        }
        return with_user_messages(output, user_messages);
    }
    if event == "Stop" && continuation {
        return with_user_messages(
            json!({"decision": "block", "reason": context.join("\n")}),
            user_messages,
        );
    }
    if event == "PostToolUse" && !reason.is_empty() {
        let mut output = json!({"decision": "block", "reason": reason});
        if !context.is_empty() {
            output["hookSpecificOutput"] = json!({
                "hookEventName": event,
                "additionalContext": context.join("\n"),
            });
        }
        return with_user_messages(output, user_messages);
    }
    if event == "Stop" && !reason.is_empty() {
        return with_user_messages(
            json!({"decision": "block", "reason": reason}),
            user_messages,
        );
    }
    if let Some(output) = replacement {
        let mut specific = json!({
            "hookEventName": event,
            "updatedToolOutput": output,
        });
        if !context.is_empty() {
            specific["additionalContext"] = Value::String(context.join("\n"));
        }
        return with_user_messages(json!({"hookSpecificOutput": specific}), user_messages);
    }
    if context.is_empty() {
        with_user_messages(json!({}), user_messages)
    } else {
        with_user_messages(
            json!({"hookSpecificOutput": {
                "hookEventName": event,
                "additionalContext": context.join("\n"),
            }}),
            user_messages,
        )
    }
}

fn with_user_messages(mut output: Value, messages: Vec<String>) -> Value {
    if !messages.is_empty() {
        output["systemMessage"] = Value::String(messages.join("\n"));
    }
    output
}

fn session_end_failure(outputs: &BTreeMap<HookEffect, Vec<PortableHookResult>>) -> Option<String> {
    outputs
        .values()
        .flat_map(|results| results.iter())
        .find_map(|result| {
            let reason = match &result.output {
                PortableHookOutput::Failure { reason, .. }
                | PortableHookOutput::FailureDeny { reason, .. }
                | PortableHookOutput::FailureContinue { reason, .. } => reason,
                _ => return None,
            };
            Some(format!("hook `{}`: {reason}", result.hook))
        })
}

fn extract_field(field: HookEventField, input: &Value) -> DaloResult<Option<String>> {
    let value = match field {
        HookEventField::SessionId => input.get("session_id"),
        HookEventField::SessionCwd => input.get("cwd"),
        HookEventField::SessionPermissionMode => input.get("permission_mode"),
        HookEventField::ActorKind => {
            return Ok(Some(if input.get("agent_id").is_some() {
                "subagent".to_owned()
            } else {
                "root".to_owned()
            }));
        }
        HookEventField::ActorId => input.get("agent_id"),
        HookEventField::TranscriptPath => input.get("transcript_path"),
        HookEventField::SessionEndReason => input.get("reason"),
        HookEventField::PromptText => input.get("prompt"),
        HookEventField::ToolCallId => input
            .get("tool_use_id")
            .or_else(|| input.get("tool_call_id")),
        HookEventField::ToolName => input.get("tool_name"),
        HookEventField::WorkflowAlreadyContinued => input.get("stop_hook_active"),
        HookEventField::WorkflowLastMessage => input.get("last_assistant_message"),
    };
    let Some(value) = value else {
        return Ok(None);
    };
    let rendered = match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => {
            return Err(DaloError::StateError {
                reason: format!(
                    "event field `{}` has an invalid scalar type",
                    field.as_str()
                ),
            });
        }
    };
    if rendered.contains('\0') || rendered.len() > 1024 * 1024 {
        return Err(DaloError::StateError {
            reason: format!(
                "event field `{}` violates its size or NUL bound",
                field.as_str()
            ),
        });
    }
    if matches!(
        field,
        HookEventField::SessionCwd | HookEventField::TranscriptPath
    ) {
        let path = Path::new(&rendered);
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(DaloError::StateError {
                reason: format!(
                    "event field `{}` is not an absolute normalized path",
                    field.as_str()
                ),
            });
        }
    }
    Ok(Some(rendered))
}

fn resolve_executable(name: &str) -> DaloResult<String> {
    let path = env::var_os("PATH").ok_or_else(|| DaloError::StateError {
        reason: format!("runtime `{name}` cannot be resolved because PATH is absent"),
    })?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file()))
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| DaloError::StateError {
            reason: format!("runtime `{name}` is unavailable"),
        })
}

fn validate_hash(value: &str) -> DaloResult<()> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(DaloError::InvalidArgument {
            reason: "projection must be a 64-character SHA-256 value".to_owned(),
        })
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    const CHATTY_DISPATCH_INPUT_PATH: &str = "DALO_CHATTY_DISPATCH_INPUT_PATH";
    const CHATTY_DISPATCH_WATCHDOG: Duration = Duration::from_secs(5);

    #[test]
    fn context_uses_the_native_event_envelope() {
        let outputs = BTreeMap::from([(
            HookEffect::AddContext,
            vec![PortableHookResult {
                hook: "local:design#hook:review".to_owned(),
                output: PortableHookOutput::AddContext {
                    context: "Review contrast.".to_owned(),
                },
            }],
        )]);
        for event in [
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "UserPromptSubmit",
        ] {
            assert_eq!(
                render_native_output(HookProvider::Codex, event, &outputs),
                json!({"hookSpecificOutput": {
                    "hookEventName": event, "additionalContext": "Review contrast.",
                }})
            );
        }
    }

    #[test]
    fn context_is_preserved_when_a_pre_tool_hook_denies() {
        let outputs = BTreeMap::from([
            (
                HookEffect::AddContext,
                vec![PortableHookResult {
                    hook: "local:context#hook:review".to_owned(),
                    output: PortableHookOutput::AddContext {
                        context: "review first".to_owned(),
                    },
                }],
            ),
            (
                HookEffect::AllowDeny,
                vec![PortableHookResult {
                    hook: "local:policy#hook:block".to_owned(),
                    output: PortableHookOutput::Deny {
                        reason: "blocked".to_owned(),
                    },
                }],
            ),
        ]);
        let output = render_native_output(HookProvider::Codex, "PreToolUse", &outputs);
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            "review first"
        );
    }

    #[test]
    fn rewrite_is_allow_with_context_and_divergence_blocks() {
        let outputs = BTreeMap::from([
            (
                HookEffect::AddContext,
                vec![PortableHookResult {
                    hook: "local:context#hook:review".to_owned(),
                    output: PortableHookOutput::AddContext {
                        context: "rewritten safely".to_owned(),
                    },
                }],
            ),
            (
                HookEffect::RewriteInput,
                vec![PortableHookResult {
                    hook: "local:rewrite#hook:command".to_owned(),
                    output: PortableHookOutput::RewriteInput {
                        input: json!({"command": "echo safe"}),
                    },
                }],
            ),
        ]);
        let output = render_native_output(HookProvider::Codex, "PreToolUse", &outputs);
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            "rewritten safely"
        );

        let divergent = BTreeMap::from([(
            HookEffect::RewriteInput,
            vec![
                PortableHookResult {
                    hook: "local:a#hook:rewrite".to_owned(),
                    output: PortableHookOutput::RewriteInput {
                        input: json!({"command": "one"}),
                    },
                },
                PortableHookResult {
                    hook: "local:b#hook:rewrite".to_owned(),
                    output: PortableHookOutput::RewriteInput {
                        input: json!({"command": "two"}),
                    },
                },
            ],
        )]);
        let output = render_native_output(HookProvider::Codex, "PreToolUse", &divergent);
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(
            output["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .is_some_and(|reason| reason.contains("divergent replacement"))
        );
    }

    #[test]
    fn claude_rewrite_preserves_the_normal_permission_flow() {
        let outputs = BTreeMap::from([(
            HookEffect::RewriteInput,
            vec![PortableHookResult {
                hook: "local:rewrite#hook:input".to_owned(),
                output: PortableHookOutput::RewriteInput {
                    input: json!({"command":"reviewed"}),
                },
            }],
        )]);
        let claude = render_native_output(HookProvider::Claude, "PreToolUse", &outputs);
        assert_eq!(
            claude["hookSpecificOutput"]["updatedInput"]["command"],
            "reviewed"
        );
        assert!(
            claude["hookSpecificOutput"]
                .get("permissionDecision")
                .is_none()
        );
        let codex = render_native_output(HookProvider::Codex, "PreToolUse", &outputs);
        assert_eq!(codex["hookSpecificOutput"]["permissionDecision"], "allow");
    }

    #[test]
    fn replacement_preserves_context_and_conflicts_stop_post_result() {
        let outputs = BTreeMap::from([
            (
                HookEffect::AddContext,
                vec![PortableHookResult {
                    hook: "local:context#hook:review".to_owned(),
                    output: PortableHookOutput::AddContext {
                        context: "inspect result".to_owned(),
                    },
                }],
            ),
            (
                HookEffect::ReplaceOutput,
                vec![PortableHookResult {
                    hook: "local:replace#hook:result".to_owned(),
                    output: PortableHookOutput::ReplaceOutput {
                        output: json!({"result": "redacted"}),
                    },
                }],
            ),
        ]);
        let output = render_native_output(HookProvider::Codex, "PostToolUse", &outputs);
        assert_eq!(
            output["hookSpecificOutput"]["updatedToolOutput"]["result"],
            "redacted"
        );
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            "inspect result"
        );

        let conflict = BTreeMap::from([(
            HookEffect::ReplaceOutput,
            vec![
                PortableHookResult {
                    hook: "local:a#hook:result".to_owned(),
                    output: PortableHookOutput::ReplaceOutput {
                        output: json!({"result": "one"}),
                    },
                },
                PortableHookResult {
                    hook: "local:b#hook:result".to_owned(),
                    output: PortableHookOutput::ReplaceOutput {
                        output: json!({"result": "two"}),
                    },
                },
            ],
        )]);
        let output = render_native_output(HookProvider::Codex, "PostToolUse", &conflict);
        assert_eq!(output["decision"], "block");
        assert!(
            output["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("divergent replacement"))
        );
    }

    #[test]
    fn failure_visibility_controls_model_context_but_keeps_user_diagnostic() {
        let user_only = BTreeMap::from([(
            HookEffect::AddContext,
            vec![PortableHookResult {
                hook: "local:review#hook:context".to_owned(),
                output: PortableHookOutput::Failure {
                    reason: "hook handler timed out".to_owned(),
                    visibility: crate::hook::HookErrorVisibility::User,
                },
            }],
        )]);
        let output = render_native_output(HookProvider::Codex, "PostToolUse", &user_only);
        assert_eq!(
            output["systemMessage"],
            "hook `local:review#hook:context`: hook handler timed out"
        );
        assert!(
            output["hookSpecificOutput"]
                .get("additionalContext")
                .is_none()
        );

        let model_and_user = BTreeMap::from([(
            HookEffect::AddContext,
            vec![PortableHookResult {
                hook: "local:review#hook:context".to_owned(),
                output: PortableHookOutput::Failure {
                    reason: "hook handler timed out".to_owned(),
                    visibility: crate::hook::HookErrorVisibility::ModelAndUser,
                },
            }],
        )]);
        let output = render_native_output(HookProvider::Codex, "PostToolUse", &model_and_user);
        assert_eq!(
            output["systemMessage"],
            "hook `local:review#hook:context`: hook handler timed out"
        );
        assert_eq!(
            output["hookSpecificOutput"]["additionalContext"],
            "hook `local:review#hook:context`: hook handler timed out"
        );
    }

    #[test]
    fn session_end_failure_is_returned_as_dispatch_error() {
        let outputs = BTreeMap::from([(
            HookEffect::Observe,
            vec![PortableHookResult {
                hook: "local:session#hook:cleanup".to_owned(),
                output: PortableHookOutput::Failure {
                    reason: "hook handler returned malformed output".to_owned(),
                    visibility: crate::hook::HookErrorVisibility::User,
                },
            }],
        )]);
        assert_eq!(
            session_end_failure(&outputs).as_deref(),
            Some("hook `local:session#hook:cleanup`: hook handler returned malformed output")
        );
        assert!(
            session_end_failure(&BTreeMap::from([(
                HookEffect::Observe,
                vec![PortableHookResult {
                    hook: "local:session#hook:cleanup".to_owned(),
                    output: PortableHookOutput::Observe,
                }],
            )]))
            .is_none()
        );
    }

    #[test]
    fn fail_closed_rewrite_failure_is_a_denial_without_a_composition_conflict() {
        let outcome = compose_results(
            HookEffect::RewriteInput,
            &[PortableHookResult {
                hook: "local:rewrite#hook:command".to_owned(),
                output: PortableHookOutput::FailureDeny {
                    reason: "hook handler returned malformed output".to_owned(),
                    visibility: crate::hook::HookErrorVisibility::ModelAndUser,
                },
            }],
        );
        assert_eq!(outcome.denials, ["hook handler returned malformed output"]);
        assert!(outcome.conflicts.is_empty());

        let user_only = BTreeMap::from([(
            HookEffect::RewriteInput,
            vec![PortableHookResult {
                hook: "local:rewrite#hook:command".to_owned(),
                output: PortableHookOutput::FailureDeny {
                    reason: "hook handler timed out".to_owned(),
                    visibility: crate::hook::HookErrorVisibility::User,
                },
            }],
        )]);
        let output = render_native_output(HookProvider::Codex, "PreToolUse", &user_only);
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "hook failure blocked this action"
        );
        assert_eq!(
            output["systemMessage"],
            "hook `local:rewrite#hook:command`: hook handler timed out"
        );
        assert!(
            !output["hookSpecificOutput"]
                .to_string()
                .contains("timed out")
        );
    }

    #[test]
    fn effect_mismatch_is_rejected_before_composition() {
        let error = validate_output(
            HookEffect::AllowDeny,
            &PortableHookOutput::AddContext {
                context: "advice".to_owned(),
            },
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("incompatible with declared effect allow_deny")
        );
    }

    fn fixture(
        script: &str,
        timeout_ms: u32,
    ) -> (tempfile::TempDir, StorePaths, crate::hook::HookStatusReport) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("store");
        crate::store::init_store(root.clone(), false).unwrap();
        let paths = StorePaths::new(root);
        let package = paths.local_dir.join("plugins/policy");
        fs::create_dir_all(package.join("bin")).unwrap();
        let manifest = r#"schema_version = 1
[plugin]
name = "policy"
description = "Shell policy"

[[tool]]
schema_version = 1
id = "check"
entry = "bin/check"
runtime = "executable"
platforms = ["macos", "linux"]
argv = []
cwd = "tool_root"
capabilities = []
availability = "required"

[[hook]]
schema_version = 1
id = "protect-shell"
tool = "check"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
matcher = { tool_names = ["Bash"] }
"#
        .replace("timeout_ms = 2000", &format!("timeout_ms = {timeout_ms}"));
        fs::write(package.join(crate::plugin::PLUGIN_FILE), manifest).unwrap();
        let entry = package.join("bin/check");
        fs::write(&entry, script).unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        crate::tool::approve(&paths, "local:policy#tool:check", false).unwrap();
        crate::hook::approve(&paths, "local:policy#hook:protect-shell", false).unwrap();
        let status = crate::hook::show(&paths, "local:policy#hook:protect-shell").unwrap();
        (temp, paths, status)
    }

    fn fixture_variant(
        script: &str,
        timeout_ms: u32,
        effect: &str,
        failure_policy: &str,
        error_visibility: &str,
    ) -> (tempfile::TempDir, StorePaths, crate::hook::HookStatusReport) {
        let (temp, paths, _status) = fixture(script, timeout_ms);
        let manifest_path = paths.local_dir.join("plugins/policy/PLUGIN.toml");
        let manifest = fs::read_to_string(&manifest_path).unwrap();
        let manifest = manifest
            .replace("effect = \"allow_deny\"", &format!("effect = \"{effect}\""))
            .replace(
                "failure_policy = \"fail_closed\"",
                &format!("failure_policy = \"{failure_policy}\""),
            )
            .replace(
                "error_visibility = \"model_and_user\"",
                &format!("error_visibility = \"{error_visibility}\""),
            );
        fs::write(&manifest_path, manifest).unwrap();
        crate::hook::approve(&paths, "local:policy#hook:protect-shell", false).unwrap();
        let status = crate::hook::show(&paths, "local:policy#hook:protect-shell").unwrap();
        (temp, paths, status)
    }

    fn compile_and_store(
        paths: &StorePaths,
        provider: HookProvider,
        status: crate::hook::HookStatusReport,
    ) -> crate::hook::NativeHookProjection {
        let projection = crate::hook::compile_native_projection(
            paths,
            provider,
            provider.baseline(),
            Path::new("/usr/bin/dalo"),
            &[status],
        )
        .unwrap();
        let sidecar = paths.root.join("native/settings.json");
        let plan =
            crate::hook_sidecar::plan_sidecar(paths, provider, &sidecar, &projection).unwrap();
        crate::hook_sidecar::apply_sidecar(paths, &projection, plan, false).unwrap();
        projection
    }

    fn stored_hook(
        paths: &StorePaths,
        projection: &crate::hook::NativeHookProjection,
    ) -> DispatcherHook {
        let manifest = fs::read(
            paths
                .hooks_dir
                .join("projections")
                .join(format!("{}.json", projection.fingerprint)),
        )
        .unwrap();
        serde_json::from_slice::<DispatcherManifest>(&manifest)
            .unwrap()
            .hooks
            .into_iter()
            .next()
            .unwrap()
    }

    #[test]
    fn dispatcher_executes_only_staged_contract_and_translates_denial() {
        let (_temp, paths, status) = fixture(
            "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"kind\":\"deny\",\"reason\":\"blocked by policy\"}'\n",
            2_000,
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "blocked by policy"
        );
    }

    #[test]
    fn malformed_required_gate_output_fails_closed() {
        let (_temp, paths, status) = fixture("#!/bin/sh\nprintf '%s' 'not-json'\n", 2_000);
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert!(
            output["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("malformed output")
        );
    }

    #[test]
    fn report_failure_reaches_user_only_without_model_context() {
        let (_temp, paths, status) = fixture_variant(
            "#!/bin/sh\nprintf '%s' 'not-json'\n",
            2_000,
            "add_context",
            "report",
            "user",
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();
        assert_eq!(
            output["systemMessage"],
            "hook `local:policy#hook:protect-shell`: hook handler returned malformed output"
        );
        assert!(
            output["hookSpecificOutput"]
                .get("additionalContext")
                .is_none()
        );
        assert!(
            output["hookSpecificOutput"]
                .get("permissionDecision")
                .is_none()
        );
    }

    #[test]
    fn incompatible_handler_output_uses_fail_closed_policy() {
        let (_temp, paths, status) = fixture_variant(
            "#!/bin/sh\nprintf '%s' '{\"kind\":\"add_context\",\"context\":\"advice\"}'\n",
            2_000,
            "allow_deny",
            "fail_closed",
            "model_and_user",
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "hook handler returned output incompatible with declared effect"
        );
    }

    #[test]
    fn dispatcher_drains_chatty_handler_before_it_consumes_large_native_input() {
        let Some(input_path) = std::env::var_os(CHATTY_DISPATCH_INPUT_PATH) else {
            return run_chatty_dispatch_in_child();
        };
        let input = fs::read(input_path).expect("child input file should be readable");
        run_chatty_dispatch(&input);
    }

    fn run_chatty_dispatch(input: &[u8]) {
        let (_temp, paths, status) = fixture(
            "#!/bin/sh\ndd if=/dev/zero bs=1048576 count=1 1>&2 2>/dev/null\ncat >/dev/null\nprintf '%s' '{\"kind\":\"deny\",\"reason\":\"blocked after input\"}'\n",
            2_000,
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            input,
        )
        .expect("chatty handler should complete");

        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "blocked after input"
        );
    }

    fn run_chatty_dispatch_in_child() {
        let input = serde_json::to_vec(&json!({
            "session_id": "s",
            "cwd": "/tmp",
            "tool_name": "Bash",
            "tool_use_id": "t",
            "padding": "x".repeat(1024 * 1024),
        }))
        .unwrap();
        let mut input_file = tempfile::NamedTempFile::new().expect("child input file should open");
        input_file
            .write_all(&input)
            .expect("child input file should be written");
        input_file
            .flush()
            .expect("child input file should be flushed");

        let executable = std::env::current_exe().expect("test executable should be available");
        let mut command = std::process::Command::new(executable);
        command
            .args([
                "--exact",
                "hook_dispatch::tests::dispatcher_drains_chatty_handler_before_it_consumes_large_native_input",
                "--nocapture",
            ])
            .env(CHATTY_DISPATCH_INPUT_PATH, input_file.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().expect("chatty dispatch child should start");
        let stdout = child
            .stdout
            .take()
            .expect("child stdout should be captured");
        let stderr = child
            .stderr
            .take()
            .expect("child stderr should be captured");
        let stdout_reader = std::thread::spawn(move || read_bounded(stdout));
        let stderr_reader = std::thread::spawn(move || read_bounded(stderr));
        let deadline = Instant::now() + CHATTY_DISPATCH_WATCHDOG;

        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    terminate_handler_process(&mut child);
                    let stdout = stdout_reader
                        .join()
                        .expect("child stdout reader should not panic")
                        .expect("child stdout should be readable");
                    let stderr = stderr_reader
                        .join()
                        .expect("child stderr reader should not panic")
                        .expect("child stderr should be readable");
                    panic!(
                        "chatty dispatch child exceeded {}: stdout={} stderr={}",
                        CHATTY_DISPATCH_WATCHDOG.as_secs_f32(),
                        String::from_utf8_lossy(&stdout),
                        String::from_utf8_lossy(&stderr),
                    );
                }
                Err(error) => {
                    terminate_handler_process(&mut child);
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    panic!("chatty dispatch child could not be polled: {error}");
                }
            }
        };
        let stdout = stdout_reader
            .join()
            .expect("child stdout reader should not panic")
            .expect("child stdout should be readable");
        let stderr = stderr_reader
            .join()
            .expect("child stderr reader should not panic")
            .expect("child stderr should be readable");
        assert!(
            status.success(),
            "chatty dispatch child failed with {status}: stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr),
        );
    }

    #[test]
    fn dispatcher_timeout_terminates_handler_that_never_reads_native_input() {
        let (_temp, paths, status) = fixture("#!/bin/sh\nsleep 10\n", 150);
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let input = serde_json::to_vec(&json!({
            "session_id": "s",
            "cwd": "/tmp",
            "tool_name": "Bash",
            "tool_use_id": "t",
            "padding": "x".repeat(1024 * 1024),
        }))
        .unwrap();

        let started = Instant::now();
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            &input,
        )
        .expect("fail-closed handler timeout should be translated to a denial");

        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "hook handler timed out"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timeout should terminate the handler and join output readers"
        );
    }

    #[test]
    fn dispatcher_terminates_background_child_holding_handler_output_pipes() {
        let (_temp, paths, status) = fixture(
            "#!/bin/sh\n/bin/sleep 10 &\nprintf '%s' 'not-json'\n",
            2_000,
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);

        let started = Instant::now();
        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .expect("failed handler output should be translated to a fail-closed denial");

        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "hook handler returned malformed output"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "dispatcher should terminate pipe-holding descendants before joining readers"
        );
    }

    #[test]
    fn verify_hook_rejects_a_tampered_contract_hash() {
        let (_temp, paths, status) = fixture("#!/bin/sh\nprintf '%s' '{}'\n", 2_000);
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let mut hook = stored_hook(&paths, &projection);
        hook.contract_hash = "0".repeat(64);

        let error = verify_hook(&paths, &hook).unwrap_err();

        assert!(error.to_string().contains("contract hash mismatch"));
    }

    #[test]
    fn bounded_reader_stops_at_the_first_over_limit_byte() {
        struct ReaderThatRejectsReadsPastLimit {
            consumed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        impl Read for ReaderThatRejectsReadsPastLimit {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let limit = MAX_HANDLER_OUTPUT as usize + 1;
                let consumed = self.consumed.load(std::sync::atomic::Ordering::SeqCst);
                if consumed >= limit {
                    return Err(std::io::Error::other(
                        "reader was consumed past the handler output cap",
                    ));
                }
                let length = buffer.len().min(limit - consumed);
                buffer[..length].fill(0);
                self.consumed
                    .fetch_add(length, std::sync::atomic::Ordering::SeqCst);
                Ok(length)
            }
        }

        let consumed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let error = read_bounded(ReaderThatRejectsReadsPastLimit {
            consumed: std::sync::Arc::clone(&consumed),
        })
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "hook output exceeds 4 MiB");
        assert_eq!(
            consumed.load(std::sync::atomic::Ordering::SeqCst),
            MAX_HANDLER_OUTPUT as usize + 1
        );
    }

    #[test]
    fn handler_output_exactly_at_the_limit_is_accepted() {
        let output =
            read_bounded(std::io::Cursor::new(vec![0; MAX_HANDLER_OUTPUT as usize])).unwrap();

        assert_eq!(output.len(), MAX_HANDLER_OUTPUT as usize);
    }

    #[test]
    fn oversized_handler_output_fails_closed_in_the_dispatcher() {
        let (_temp, paths, status) = fixture(
            "#!/bin/sh\n/bin/dd if=/dev/zero bs=4194305 count=1 2>/dev/null\n",
            2_000,
        );
        let projection = compile_and_store(&paths, HookProvider::Claude, status);

        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Claude,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();

        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "hook handler failed validation or execution"
        );
    }

    #[test]
    fn dispatcher_translates_codex_provider_dispatches() {
        let (_temp, paths, status) = fixture(
            "#!/bin/sh\nprintf '%s' '{\"kind\":\"deny\",\"reason\":\"blocked by Codex policy\"}'\n",
            2_000,
        );
        let projection = compile_and_store(&paths, HookProvider::Codex, status);

        let output = dispatch(
            &paths,
            &DispatchRequest {
                provider: HookProvider::Codex,
                projection: &projection.fingerprint,
                event: "PreToolUse",
                group: "group-0000",
            },
            br#"{"session_id":"s","cwd":"/tmp","tool_name":"Bash","tool_use_id":"t"}"#,
        )
        .unwrap();

        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            output["hookSpecificOutput"]["permissionDecisionReason"],
            "blocked by Codex policy"
        );
    }
}
