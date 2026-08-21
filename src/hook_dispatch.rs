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
use rustix::process::{Pid, Signal, kill_process_group};

use crate::error::{DaloError, DaloResult};
use crate::hook::{
    HookDescriptorV1, HookEffect, HookEventField, HookFailurePolicy, HookProvider,
    PortableHookOutput, PortableHookResult, compose_results,
};
use crate::plugin::{ToolRecord, ToolRuntime};
use crate::store::StorePaths;
use crate::tool;

const MAX_HANDLER_OUTPUT: u64 = 4 * 1024 * 1024;

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
    Ok(render_native_output(request.event, &outputs))
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
    Ok(output)
}

fn terminate_handler_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = Pid::from_child(child);
        if kill_process_group(process_group, Signal::KILL).is_ok() {
            let _ = child.wait();
            return;
        }
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
    } else {
        "hook handler failed validation or execution"
    }
    .to_owned();
    match (hook.descriptor.effect, hook.descriptor.failure_policy) {
        (HookEffect::AllowDeny | HookEffect::RewriteInput, HookFailurePolicy::FailClosed) => {
            Ok(PortableHookOutput::Deny { reason: bounded })
        }
        (HookEffect::ContinueWorkflow, HookFailurePolicy::FailClosed) => {
            Ok(PortableHookOutput::ContinueWorkflow { reason: bounded })
        }
        (_, HookFailurePolicy::FailOpen | HookFailurePolicy::Report) => {
            Ok(PortableHookOutput::Observe)
        }
        _ => Err(DaloError::StateError { reason: bounded }),
    }
}

fn render_native_output(
    event: &str,
    outputs: &BTreeMap<HookEffect, Vec<PortableHookResult>>,
) -> Value {
    let mut context = Vec::new();
    let mut denials = Vec::new();
    let mut rewrite = None;
    let mut replacement = None;
    let mut continuation = false;
    let mut conflicts = Vec::new();
    for (effect, results) in outputs {
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
            return json!({"hookSpecificOutput": {
                "hookEventName": event,
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            }});
        }
        if let Some(input) = rewrite {
            return json!({"hookSpecificOutput": {
                "hookEventName": event,
                "updatedInput": input,
                "additionalContext": context.join("\n"),
            }});
        }
    }
    if event == "UserPromptSubmit" && !reason.is_empty() {
        return json!({"decision": "block", "reason": reason});
    }
    if event == "Stop" && continuation {
        return json!({"decision": "block", "reason": context.join("\n")});
    }
    if let Some(output) = replacement {
        return json!({"hookSpecificOutput": {
            "hookEventName": event,
            "updatedToolOutput": output,
        }});
    }
    if context.is_empty() {
        json!({})
    } else {
        json!({"additionalContext": context.join("\n")})
    }
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
    fn verify_hook_rejects_a_tampered_contract_hash() {
        let (_temp, paths, status) = fixture("#!/bin/sh\nprintf '%s' '{}'\n", 2_000);
        let projection = compile_and_store(&paths, HookProvider::Claude, status);
        let mut hook = stored_hook(&paths, &projection);
        hook.contract_hash = "0".repeat(64);

        let error = verify_hook(&paths, &hook).unwrap_err();

        assert!(error.to_string().contains("contract hash mismatch"));
    }

    #[test]
    fn oversized_handler_output_is_rejected_without_buffering_past_the_limit() {
        let error = read_bounded(std::io::Cursor::new(vec![
            0;
            MAX_HANDLER_OUTPUT as usize + 1
        ]))
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "hook output exceeds 4 MiB");
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
