//! Narrow wrapper around the system `git` command.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process_group};
use tempfile::NamedTempFile;

use crate::error::{DaloError, DaloResult};

const GIT_LOCAL_TIMEOUT: Duration = Duration::from_secs(60);
const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(300);
// Most local Git commands finish well below 50 ms. Poll at 1 ms so process
// collection does not impose a fixed latency floor on every command.
const GIT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const GIT_TIMEOUT_ENV: &str = "DALO_GIT_TIMEOUT_SECS";
const MAX_CONCURRENT_FETCHES: usize = 4;
const GIT_ALLOWED_PROTOCOLS: &str = "https:ssh:git:file";

type SshPreflightCache = Mutex<BTreeMap<(String, PathBuf), Arc<OnceLock<bool>>>>;
static SSH_PREFLIGHT_CACHE: OnceLock<SshPreflightCache> = OnceLock::new();

/// Run `git init` in the provided directory.
pub fn init_repo(path: &Path) -> DaloResult<()> {
    run_git(path, &["init", "-q"]).map(|_| ())
}

/// Clone a Git repository.
pub fn clone_repo(url: &str, destination: &Path) -> DaloResult<()> {
    validate_remote_url(url)?;
    let cwd = destination.parent().unwrap_or_else(|| Path::new("."));
    preflight_clone_source(url, cwd)?;
    let destination_arg = destination.to_string_lossy().into_owned();
    print_network_progress(&format!(
        "Cloning repository `{}`...",
        display_git_value(url)
    ));
    // `--` terminates option parsing so a user-supplied URL that looks like a
    // flag (e.g. `--upload-pack=...`) can never be treated as a git option.
    run_git_network(cwd, &["clone", "--quiet", "--", url, &destination_arg]).map(|_| ())
}

/// Check local clone sources before a later clone would mutate its destination.
///
/// Remote locations remain deliberately offline here: team manifest authoring
/// must work while disconnected, and `sync` performs the authenticated remote
/// check before it creates a managed checkout.
pub fn preflight_clone_source(location: &str, cwd: &Path) -> DaloResult<()> {
    preflight_local_clone_source(location, cwd)
}

/// Return whether `path` is inside a non-bare Git worktree.
pub fn is_worktree(path: &Path) -> DaloResult<bool> {
    match run_git(path, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(output) => Ok(output.trim() == "true"),
        Err(DaloError::CommandFailed { status, .. }) if status == "128" => Ok(false),
        Err(error) => Err(error),
    }
}

/// Reject unsafe Git transports and URLs that embed credentials.
///
/// Local paths remain valid clone sources. Remote sources must use one of the
/// protocols that Dalo supports, or a constrained SCP-style SSH location.
pub fn validate_remote_url(url: &str) -> DaloResult<()> {
    if url_has_forbidden_userinfo(url) || !uses_allowed_git_transport(url) {
        return Err(DaloError::UnsafeRemoteUrl);
    }
    Ok(())
}

/// Return a remote location that is safe to include in human or JSON output.
#[must_use]
pub fn display_remote_url(url: &str) -> String {
    redact_url_userinfo(url)
}

/// Update the current tracking branch through a fast-forward-only pull.
pub fn pull_ff_only(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!("Refreshing source `{}`...", path.display()));
    run_git_network(path, &["pull", "--ff-only", "--quiet"]).map(|_| ())
}

/// Return whether a checkout has local changes to tracked files.
///
/// Untracked files are ignored: a fast-forward or reset never destroys them, so
/// a stray file (for example macOS `.DS_Store`) must not make a dalo-managed
/// checkout look dirty and block refresh or sync. Only tracked modifications,
/// staged changes, and unresolved merges count.
pub fn is_dirty(path: &Path) -> DaloResult<bool> {
    let output = run_git(path, &["status", "--porcelain=v2", "--untracked-files=no"])?;
    Ok(!output.trim().is_empty())
}

/// Return whether `file` is tracked by the checkout's current index.
///
/// Callers that bind persisted provenance to `HEAD` must reject untracked
/// files even though [`is_dirty`] intentionally ignores them for refreshes.
pub fn is_tracked_file(repo: &Path, file: &Path) -> DaloResult<bool> {
    let relative = file
        .strip_prefix(repo)
        .map_err(|_| DaloError::InvalidArgument {
            reason: format!(
                "path `{}` is outside source checkout `{}`",
                file.display(),
                repo.display()
            ),
        })?;
    let relative = relative
        .to_str()
        .ok_or_else(|| DaloError::InvalidArgument {
            reason: format!("source-relative path `{}` is not UTF-8", relative.display()),
        })?;
    match run_git(repo, &["ls-files", "--error-unmatch", "--", relative]) {
        Ok(_) => Ok(true),
        Err(DaloError::CommandFailed { status, .. }) if status == "1" => Ok(false),
        Err(error) => Err(error),
    }
}

/// Return the current HEAD commit.
pub fn rev_parse_head(path: &Path) -> DaloResult<String> {
    run_git(path, &["rev-parse", "HEAD"]).map(|output| output.trim().to_owned())
}

/// Read one UTF-8 file from an immutable commit without changing the checkout.
///
/// Persisted instruction provenance uses full commit hashes. Restricting both
/// the revision and path keeps the resulting `git show <commit>:<path>` object
/// spec unambiguous even when lock data has been edited externally.
pub fn read_file_at_commit(repo: &Path, commit: &str, relative_path: &Path) -> DaloResult<String> {
    if !matches!(commit.len(), 40 | 64)
        || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        || relative_path.as_os_str().is_empty()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(DaloError::StateError {
            reason: "instruction lock contains invalid Git provenance".to_owned(),
        });
    }
    let relative = relative_path
        .to_str()
        .ok_or_else(|| DaloError::StateError {
            reason: format!(
                "instruction source path `{}` is not UTF-8",
                relative_path.display()
            ),
        })?;
    let object = format!("{commit}:{relative}");
    run_git(repo, &["show", &object])
}

/// Resolve a fixed revision (such as `FETCH_HEAD`) to a commit hash.
pub fn rev_parse(path: &Path, revision: &str) -> DaloResult<String> {
    run_git(path, &["rev-parse", revision]).map(|output| output.trim().to_owned())
}

/// Read-only fetch of the remote's HEAD. Records it in `FETCH_HEAD` without
/// moving the working tree.
pub fn fetch(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!(
        "Checking upstream drift for `{}`...",
        path.display()
    ));
    run_git_network(path, &["fetch", "--quiet", "origin", "HEAD"]).map(|_| ())
}

/// Fetch the configured upstream without moving the current checkout.
pub fn fetch_upstream(path: &Path) -> DaloResult<()> {
    print_network_progress(&format!(
        "Staging source refresh for `{}`...",
        path.display()
    ));
    run_git_network(path, &["fetch", "--quiet"]).map(|_| ())
}

/// Fetch several independent checkouts with a fixed per-batch concurrency cap.
///
/// Results retain the exact input ordering even when commands finish out of
/// order. Scoped workers are always joined before this function returns.
pub(crate) fn fetch_upstreams_bounded(paths: &[PathBuf]) -> Vec<DaloResult<()>> {
    for path in paths {
        print_network_progress(&format!(
            "Staging source refresh for `{}`...",
            path.display()
        ));
    }
    run_bounded(paths, MAX_CONCURRENT_FETCHES, |path| {
        run_git_network(path, &["fetch", "--quiet"]).map(|_| ())
    })
}

fn run_bounded<T, F>(items: &[T], limit: usize, operation: F) -> Vec<DaloResult<()>>
where
    T: Sync,
    F: Fn(&T) -> DaloResult<()> + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }
    let worker_count = limit.max(1).min(items.len());
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let next = &next;
            let operation = &operation;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    if sender.send((index, operation(item))).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(sender);
    let mut results = receiver.into_iter().collect::<Vec<_>>();
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

/// Resolve a manifest-declared revision to a concrete commit.
///
/// Remote branches are preferred over local branches so a freshly fetched
/// branch name cannot accidentally resolve to a stale local tracking branch.
pub fn resolve_manifest_revision(path: &Path, revision: &str) -> DaloResult<String> {
    validate_manifest_revision(revision)?;

    let remote = format!("refs/remotes/origin/{revision}^{{commit}}");
    match rev_parse(path, &remote) {
        Ok(commit) => Ok(commit),
        Err(_) => {
            let requested = format!("{revision}^{{commit}}");
            rev_parse(path, &requested)
        }
    }
}

/// Validate a human-authored manifest revision before it reaches Git.
pub fn validate_manifest_revision(revision: &str) -> DaloResult<()> {
    // A manifest pin must name a single concrete commit, tag, or ref -- not a
    // Git revision expression. Reject empty/flag-like/whitespace values and the
    // range (`..`), reflog (`@{`), ancestry (`^`, `~`), and refspec/glob
    // (`:`, `?`, `*`, `[`, `\`) operators, plus control characters. This still
    // accepts commit hashes, `v1.0.0`, `main`, and `release/2024`.
    let has_operator = revision.contains(['^', '~', ':', '?', '*', '[', '\\']);
    if revision.is_empty()
        || revision.starts_with('-')
        || revision
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || revision.contains("..")
        || revision.contains("@{")
        || has_operator
    {
        Err(DaloError::CheckFailed {
            reason: format!("invalid manifest Git revision `{revision}`"),
        })
    } else {
        Ok(())
    }
}

/// Move a clean managed checkout to an exact detached commit.
pub fn checkout_detached(path: &Path, commit: &str) -> DaloResult<()> {
    run_git(
        path,
        &["checkout", "--detach", "--force", "--quiet", commit],
    )
    .map(|_| ())
}

/// Count commits reachable from `to` but not from `from`.
pub fn revision_count(path: &Path, from: &str, to: &str) -> DaloResult<usize> {
    let range = format!("{from}..{to}");
    let output = run_git(path, &["rev-list", "--count", &range])?;
    output.trim().parse().map_err(|error| {
        DaloError::Io(std::io::Error::other(format!(
            "git returned an invalid revision count `{}`: {error}",
            output.trim()
        )))
    })
}

/// Fast-forward the current branch to an already fetched revision.
pub fn fast_forward_to(path: &Path, revision: &str) -> DaloResult<()> {
    run_git(path, &["merge", "--ff-only", "--quiet", revision]).map(|_| ())
}

/// Restore a clean managed checkout to a previously recorded commit.
///
/// Callers must verify that the checkout is clean before beginning the
/// transaction. This is intentionally reserved for command-level rollback.
pub fn reset_hard_to(path: &Path, revision: &str) -> DaloResult<()> {
    run_git(path, &["reset", "--hard", "--quiet", revision]).map(|_| ())
}

/// Check a commit out into a detached worktree for read-only inspection. The
/// caller's own checkout (and pin) is left untouched.
pub fn add_detached_worktree(repo: &Path, dest: &Path, commit: &str) -> DaloResult<()> {
    let dest_arg = dest.to_string_lossy().into_owned();
    run_git(
        repo,
        &["worktree", "add", "--detach", "--quiet", &dest_arg, commit],
    )
    .map(|_| ())
}

/// Remove a worktree created with [`add_detached_worktree`].
pub fn remove_worktree(repo: &Path, dest: &Path) -> DaloResult<()> {
    let dest_arg = dest.to_string_lossy().into_owned();
    run_git(repo, &["worktree", "remove", "--force", &dest_arg]).map(|_| ())
}

/// Prune stale Git worktree administrative records.
pub fn prune_worktrees(repo: &Path) -> DaloResult<()> {
    run_git(repo, &["worktree", "prune"]).map(|_| ())
}

fn run_git(path: &Path, args: &[&str]) -> DaloResult<String> {
    run_git_program("git", path, args, git_timeout(GIT_LOCAL_TIMEOUT))
}

fn run_git_network(path: &Path, args: &[&str]) -> DaloResult<String> {
    run_git_program_with_ssh_preflight("git", path, args, git_timeout(GIT_NETWORK_TIMEOUT), true)
}

fn run_git_program(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
) -> DaloResult<String> {
    run_git_program_with_ssh_preflight(program, path, args, timeout, false)
}

fn run_git_program_with_ssh_preflight(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
    preflight_core_ssh_command: bool,
) -> DaloResult<String> {
    let ssh_command_env = std::env::var_os("GIT_SSH_COMMAND");
    run_git_program_with_ssh_preflight_and_env(
        program,
        path,
        args,
        timeout,
        preflight_core_ssh_command,
        ssh_command_env.as_deref(),
    )
}

fn run_git_program_with_ssh_preflight_and_env(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
    preflight_core_ssh_command: bool,
    ssh_command_env: Option<&OsStr>,
) -> DaloResult<String> {
    let core_ssh_command_configured = preflight_core_ssh_command
        && ssh_command_env.is_none()
        && cached_has_core_ssh_command(program, path, timeout.min(git_timeout(GIT_LOCAL_TIMEOUT)));
    run_git_program_with_options(
        program,
        path,
        args,
        timeout,
        ssh_command_env,
        core_ssh_command_configured,
    )
}

fn run_git_program_with_options(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
    ssh_command_env: Option<impl AsRef<OsStr>>,
    core_ssh_command_configured: bool,
) -> DaloResult<String> {
    let stdout = NamedTempFile::new()?;
    let stderr = NamedTempFile::new()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(path)
        // Do not let a configured remote select arbitrary Git transport
        // helpers. `GIT_PROTOCOL_FROM_USER=0` also prevents Git from treating
        // URLs supplied on the command line as user-approved protocols.
        .env("GIT_ALLOW_PROTOCOL", GIT_ALLOWED_PROTOCOLS)
        .env("GIT_PROTOCOL_FROM_USER", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?));
    // Unit tests invoke this production path in-process rather than through
    // the integration-test command wrapper. Keep those Git calls hermetic
    // without changing how released binaries honor user Git configuration.
    #[cfg(test)]
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS");
    configure_ssh_command(
        &mut command,
        ssh_command_env.as_ref(),
        core_ssh_command_configured,
    );
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_git_process(&mut child);
                return Err(DaloError::Io(error));
            }
        }

        let elapsed = start.elapsed();
        if elapsed >= timeout {
            terminate_git_process(&mut child);
            let failure = humanize_git_failure(args, &timeout_stderr(&stderr));
            return Err(DaloError::CommandFailed {
                program: program.to_owned(),
                args: display_git_args(args),
                status: format!("timed out after {}", format_duration(timeout)),
                summary: failure.summary,
                stderr: failure.stderr,
            });
        }

        thread::sleep(GIT_POLL_INTERVAL.min(timeout - elapsed));
    };
    let stdout_text = read_tempfile_lossy(&stdout);
    let stderr_text = read_tempfile_lossy(&stderr).trim().to_owned();

    if status.success() {
        return Ok(stdout_text);
    }

    let failure = humanize_git_failure(args, &stderr_text);
    Err(DaloError::CommandFailed {
        program: program.to_owned(),
        args: display_git_args(args),
        status: status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
        summary: failure.summary,
        stderr: failure.stderr,
    })
}

/// Terminate Git and any remote helpers it spawned, then reap the direct child.
fn terminate_git_process(child: &mut Child) {
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

fn print_network_progress(message: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("{message}");
    }
}

struct HumanizedGitFailure {
    summary: Option<String>,
    stderr: String,
}

fn humanize_git_failure(args: &[&str], stderr: &str) -> HumanizedGitFailure {
    let raw = redact_urls_in_text(stderr.trim());
    let Some(summary) = git_failure_summary(args) else {
        return HumanizedGitFailure {
            summary: None,
            stderr: raw,
        };
    };
    if raw.is_empty() {
        return HumanizedGitFailure {
            summary: Some(summary),
            stderr: String::new(),
        };
    }
    HumanizedGitFailure {
        summary: Some(summary),
        stderr: raw,
    }
}

fn git_failure_summary(args: &[&str]) -> Option<String> {
    match args {
        ["clone", .., "--", url, _destination] if looks_like_remote_location(url) => {
            Some(format!(
                "Could not clone repository `{}`. Check the URL, network/proxy access, and repository permissions.",
                display_git_value(url)
            ))
        }
        ["clone", .., "--", url, _destination] => Some(format!(
            "Could not clone local repository `{}`. Check that the path is readable and points to a Git repository.",
            display_git_value(url)
        )),
        ["pull", ..] => Some(
            "Could not refresh this source. Check network/proxy access, repository permissions, and whether the tracking branch can fast-forward."
                .to_owned(),
        ),
        ["fetch", ..] => Some(
            "Could not check the repository for upstream changes. Check network/proxy access and repository permissions."
                .to_owned(),
        ),
        _ => None,
    }
}

fn display_git_args(args: &[&str]) -> String {
    args.iter()
        .map(|arg| display_git_value(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn preflight_local_clone_source(location: &str, cwd: &Path) -> DaloResult<()> {
    let path = Path::new(location);
    let local_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    if looks_like_remote_location(location) && !local_path.exists() {
        return Ok(());
    }

    let metadata = match fs::metadata(&local_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DaloError::InvalidArgument {
                reason: format!(
                    "local source path `{}` does not exist",
                    display_git_value(location)
                ),
            });
        }
        Err(error) => return Err(DaloError::Io(error)),
    };
    // Git accepts bundle files as clone sources. Let Git validate regular
    // files so the preflight does not reject a cloneable local transport.
    if metadata.is_file() {
        return Ok(());
    }
    let has_worktree_metadata = local_path.join(".git").try_exists()?;
    let has_bare_metadata = local_path.join("HEAD").is_file()
        && local_path.join("objects").is_dir()
        && local_path.join("refs").is_dir();
    if !metadata.is_dir() || (!has_worktree_metadata && !has_bare_metadata) {
        return Err(DaloError::InvalidArgument {
            reason: format!(
                "local source path `{}` is not a Git repository (missing .git)",
                display_git_value(location)
            ),
        });
    }
    Ok(())
}

pub(crate) fn looks_like_remote_location(location: &str) -> bool {
    if Path::new(location).is_absolute() {
        return false;
    }
    if location.contains("://") {
        return true;
    }
    let Some(colon) = location.find(':') else {
        return false;
    };
    colon > 0
        && !location[..colon].contains('/')
        && location
            .get(colon + 1..)
            .is_some_and(|suffix| !suffix.is_empty())
}

fn display_git_value(value: &str) -> String {
    redact_url_userinfo(value)
        .chars()
        .fold(String::new(), |mut escaped, character| {
            if character.is_control() {
                escaped.extend(character.escape_default());
            } else {
                escaped.push(character);
            }
            escaped
        })
}

fn uses_allowed_git_transport(url: &str) -> bool {
    let Some((scheme, remainder)) = url.split_once("://") else {
        return !looks_like_remote_location(url) || is_scp_style_git_url(url);
    };

    match scheme.to_ascii_lowercase().as_str() {
        "https" | "ssh" | "git" => has_url_authority(remainder),
        // `file:///path` has an empty authority but is a valid local URL.
        "file" => !remainder.is_empty(),
        _ => false,
    }
}

fn has_url_authority(remainder: &str) -> bool {
    remainder
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| !authority.is_empty())
}

fn is_scp_style_git_url(url: &str) -> bool {
    let Some((authority, path)) = url.split_once(':') else {
        return false;
    };
    if authority.is_empty()
        || path.is_empty()
        || path.starts_with(':')
        || path.contains(':')
        || path
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return false;
    }

    let (user, host) = authority
        .split_once('@')
        .map_or((None, authority), |(user, host)| (Some(user), host));
    user.is_none_or(|user| {
        !user.is_empty()
            && user
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    }) && !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-".contains(character))
}

fn url_has_forbidden_userinfo(url: &str) -> bool {
    let Some(scheme_end) = url.find("://") else {
        return false;
    };
    let authority = &url[scheme_end + 3..];
    let authority_end = authority
        .find(|character: char| ['/', '?', '#'].contains(&character))
        .unwrap_or(authority.len());
    let authority = &authority[..authority_end];
    let Some(userinfo_end) = authority.rfind('@') else {
        return false;
    };

    // An SSH username is an address, not an embedded credential. All other
    // URL userinfo, and passwords in SSH URLs, remain forbidden.
    !url[..scheme_end].eq_ignore_ascii_case("ssh")
        || authority[..userinfo_end].is_empty()
        || authority[..userinfo_end].contains(':')
}

fn redact_url_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority = &url[authority_start..];
    let authority_end = authority
        .find(|character: char| ['/', '?', '#'].contains(&character))
        .unwrap_or(authority.len());
    let Some(userinfo_end) = authority[..authority_end].rfind('@') else {
        return url.to_owned();
    };

    format!(
        "{}***@{}",
        &url[..authority_start],
        &authority[userinfo_end + 1..]
    )
}

fn redact_urls_in_text(text: &str) -> String {
    text.split_whitespace()
        .map(redact_url_userinfo)
        .collect::<Vec<_>>()
        .join(" ")
}

fn configure_ssh_command(
    command: &mut Command,
    ssh_command_env: Option<&impl AsRef<OsStr>>,
    core_ssh_command_configured: bool,
) {
    if let Some(value) = ssh_command_env {
        command.env("GIT_SSH_COMMAND", value.as_ref());
    } else if !core_ssh_command_configured {
        command.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    } else {
        command.env_remove("GIT_SSH_COMMAND");
    }
}

fn has_core_ssh_command(program: &str, path: &Path, timeout: Duration) -> bool {
    run_git_program_with_options(
        program,
        path,
        &["config", "--get", "core.sshCommand"],
        timeout,
        Option::<&str>::None,
        true,
    )
    .is_ok()
}

fn cached_has_core_ssh_command(program: &str, path: &Path, timeout: Duration) -> bool {
    let comparable_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let cache = SSH_PREFLIGHT_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let cell = {
        let mut cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache
            .entry((program.to_owned(), comparable_path))
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone()
    };
    *cell.get_or_init(|| has_core_ssh_command(program, path, timeout))
}

fn git_timeout(default: Duration) -> Duration {
    std::env::var(GIT_TIMEOUT_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn read_tempfile_lossy(file: &NamedTempFile) -> String {
    fs::read(file.path())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn timeout_stderr(stderr: &NamedTempFile) -> String {
    let text = read_tempfile_lossy(stderr).trim().to_owned();
    if text.is_empty() {
        "git command timed out; terminal prompts are disabled".to_owned()
    } else {
        text
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}s", duration.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    #[test]
    fn run_git_program_should_report_missing_binary() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");

        let error = run_git_program(
            "dalo-definitely-missing-git-binary",
            temp_dir.path(),
            &["--version"],
            Duration::from_millis(10),
        )
        .expect_err("missing binary should fail");

        assert!(matches!(error, DaloError::Io(_)));
    }

    #[test]
    fn run_git_program_should_disable_interactive_prompts() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf 'prompt=%s ssh=%s protocols=%s from_user=%s global=%s nosystem=%s count=%s parameters=%s\\n' \"$GIT_TERMINAL_PROMPT\" \"$GIT_SSH_COMMAND\" \"$GIT_ALLOW_PROTOCOL\" \"$GIT_PROTOCOL_FROM_USER\" \"$GIT_CONFIG_GLOBAL\" \"$GIT_CONFIG_NOSYSTEM\" \"${GIT_CONFIG_COUNT:-unset}\" \"${GIT_CONFIG_PARAMETERS:-unset}\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["pull"],
            Duration::from_secs(3),
            Option::<&str>::None,
            false,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("prompt=0"));
        assert!(stderr.contains("BatchMode=yes"));
        assert!(stderr.contains("protocols=https:ssh:git:file"));
        assert!(stderr.contains("from_user=0"));
        assert!(stderr.contains("global=/dev/null"));
        assert!(stderr.contains("nosystem=1"));
        assert!(stderr.contains("count=unset"));
        assert!(stderr.contains("parameters=unset"));
    }

    #[test]
    fn run_git_program_should_preserve_user_ssh_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf 'ssh=%s\\n' \"$GIT_SSH_COMMAND\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["fetch"],
            Duration::from_secs(3),
            Some("ssh -i deploy-key -oBatchMode=yes"),
            false,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("ssh=ssh -i deploy-key -oBatchMode=yes"));
    }

    #[test]
    fn run_git_program_should_not_override_core_ssh_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nprintf \"ssh=${GIT_SSH_COMMAND:-unset}\\n\" >&2\nexit 2\n",
        );

        let error = run_git_program_with_options(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["fetch"],
            Duration::from_secs(3),
            Option::<&str>::None,
            true,
        )
        .expect_err("fake git should fail");

        let DaloError::CommandFailed { stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(stderr.contains("ssh=unset"));
        assert!(!stderr.contains("BatchMode=yes"));
    }

    #[test]
    fn run_git_program_should_timeout_hung_command() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nwhile :; do :; done\n",
        );

        let error = run_git_program(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["pull"],
            Duration::from_millis(10),
        )
        .expect_err("hung command should time out");

        let DaloError::CommandFailed { status, stderr, .. } = error else {
            panic!("expected command failure");
        };
        assert!(status.contains("timed out after"));
        assert!(stderr.contains("terminal prompts are disabled"));
    }

    #[test]
    fn run_git_program_should_skip_ssh_preflight_for_local_commands() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nif [ \"$1\" = config ]; then : > preflight-called; exit 1; fi\nprintf 'ok\\n'\n",
        );

        let output = run_git_program(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["status"],
            Duration::from_secs(3),
        )
        .expect("local command should run");

        assert_eq!(output.trim(), "ok");
        assert!(!temp_dir.path().join("preflight-called").exists());
    }

    #[test]
    fn run_git_program_should_terminate_helper_processes_on_timeout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let helper_activity_file = temp_dir.path().join("helper-activity");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nwhile :; do printf . >> \"$1\"; sleep 0.01; done &\nwhile :; do sleep 1; done\n",
        );
        let helper_activity_file_arg = helper_activity_file
            .to_str()
            .expect("helper activity path should be utf-8");

        // Tests spawn processes concurrently, and a child forked by another
        // test can still hold the freshly written script open until it execs,
        // which fails this spawn with ETXTBSY before any timeout runs. That is
        // a property of the test harness, not of the timeout, so retry the
        // spawn within a bounded window and let anything else through.
        let spawn_deadline = Instant::now() + Duration::from_secs(2);
        let error = loop {
            match run_git_program_with_options(
                fake_git.to_str().expect("script path should be utf-8"),
                temp_dir.path(),
                &[helper_activity_file_arg],
                Duration::from_secs(3),
                Option::<&str>::None,
                false,
            ) {
                Err(DaloError::Io(io_error))
                    if io_error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && Instant::now() < spawn_deadline =>
                {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => break error,
                Ok(output) => panic!("hung command should time out, got output {output:?}"),
            }
        };

        // Only the timeout branch terminates the process group, and only it
        // reports this status, so the status proves that termination ran.
        let DaloError::CommandFailed { status, .. } = error else {
            panic!("hung command should time out with a command failure, got: {error}");
        };
        assert!(
            status.contains("timed out after"),
            "expected a timeout status, got {status:?}"
        );

        // A live helper appends every 10 ms, so the file only stops growing
        // once the helper is dead. A write already in flight when the group
        // was killed may still land, so wait for a full quiet window instead
        // of comparing two fixed snapshots, and bound the wait so a helper
        // that survived fails the test rather than hanging it.
        let quiet_window = Duration::from_millis(100);
        let quiet_deadline = Instant::now() + Duration::from_secs(2);
        let mut activity = fs::read(&helper_activity_file)
            .expect("helper should record activity before the timeout");
        loop {
            thread::sleep(quiet_window);
            let now =
                fs::read(&helper_activity_file).expect("helper activity should remain readable");
            if now == activity {
                break;
            }
            assert!(
                Instant::now() < quiet_deadline,
                "timeout should terminate the helper process; it kept writing for 2s"
            );
            activity = now;
        }
    }

    #[test]
    fn core_ssh_preflight_should_share_the_git_timeout() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git",
            "#!/bin/sh\nif [ \"$1\" = config ]; then while :; do :; done; fi\nprintf 'ok\\n'\n",
        );
        let start = Instant::now();

        let output = run_git_program_with_ssh_preflight(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["--version"],
            Duration::from_secs(1),
            true,
        )
        .expect("main command should run after the timed-out preflight");

        assert_eq!(output.trim(), "ok");
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn core_ssh_preflight_should_run_once_per_repository() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git-once",
            "#!/bin/sh\nprintf '%s prompt=%s ssh=%s\\n' \"$1\" \"$GIT_TERMINAL_PROMPT\" \"$GIT_SSH_COMMAND\" >> invocations\n[ \"$1\" != config ]\n",
        );
        let program = fake_git.to_str().expect("script path should be utf-8");

        for _ in 0..2 {
            run_git_program_with_ssh_preflight_and_env(
                program,
                temp_dir.path(),
                &["fetch"],
                Duration::from_secs(3),
                true,
                None,
            )
            .expect("network command should run");
        }

        let invocations = fs::read_to_string(temp_dir.path().join("invocations"))
            .expect("shim log should be readable");
        assert_eq!(invocations.matches("config ").count(), 1);
        assert_eq!(invocations.matches("fetch ").count(), 2);
        assert!(invocations.lines().all(|line| line.contains("prompt=0")));
        assert!(
            invocations
                .lines()
                .filter(|line| line.starts_with("fetch "))
                .all(|line| line.contains("BatchMode=yes"))
        );
    }

    #[test]
    fn explicit_ssh_command_should_skip_the_redundant_preflight() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git-explicit-ssh",
            "#!/bin/sh\nprintf '%s ssh=%s\\n' \"$1\" \"$GIT_SSH_COMMAND\" >> invocations\n[ \"$1\" != config ]\n",
        );

        run_git_program_with_ssh_preflight_and_env(
            fake_git.to_str().expect("script path should be utf-8"),
            temp_dir.path(),
            &["fetch"],
            Duration::from_secs(3),
            true,
            Some(OsStr::new("ssh -i explicit-key -oBatchMode=yes")),
        )
        .expect("network command should preserve the explicit SSH policy");

        let invocations = fs::read_to_string(temp_dir.path().join("invocations"))
            .expect("shim log should be readable");
        assert!(!invocations.contains("config "));
        assert!(invocations.contains("fetch ssh=ssh -i explicit-key -oBatchMode=yes"));
    }

    #[test]
    fn bounded_git_shim_should_cap_concurrency_and_preserve_result_order() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let fake_git = write_executable(
            temp_dir.path(),
            "fake-git-parallel",
            "#!/bin/sh\nsleep 0.03\ncase \"$PWD\" in *job-1|*job-5) exit 7;; esac\n",
        );
        let jobs = (0..7)
            .map(|index| {
                let path = temp_dir.path().join(format!("job-{index}"));
                fs::create_dir(&path).expect("job directory should be created");
                path
            })
            .collect::<Vec<_>>();
        let active = AtomicUsize::new(0);
        let maximum = AtomicUsize::new(0);

        let results = run_bounded(&jobs, 3, |path| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(current, Ordering::SeqCst);
            let result = run_git_program_with_options(
                fake_git.to_str().expect("script path should be utf-8"),
                path,
                &["fetch"],
                Duration::from_secs(3),
                Option::<&str>::None,
                false,
            )
            .map(|_| ());
            active.fetch_sub(1, Ordering::SeqCst);
            result
        });

        assert!(maximum.load(Ordering::SeqCst) >= 2);
        assert!(maximum.load(Ordering::SeqCst) <= 3);
        assert_eq!(results.len(), jobs.len());
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result.is_err(), matches!(index, 1 | 5));
        }
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn git_poll_interval_should_not_add_a_50ms_latency_floor() {
        assert!(GIT_POLL_INTERVAL < Duration::from_millis(50));
    }

    #[test]
    fn humanize_git_failure_should_explain_clone_errors() {
        let message = humanize_git_failure(
            &[
                "clone",
                "--quiet",
                "--",
                "https://example.invalid/repo.git",
                "/tmp/checkout",
            ],
            "fatal: unable to access repository",
        );

        assert!(
            message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("Could not clone repository"))
        );
        assert!(
            message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("https://example.invalid/repo.git"))
        );
        assert_eq!(message.stderr, "fatal: unable to access repository");
    }

    #[test]
    fn humanize_git_failure_should_not_give_network_advice_for_local_clones() {
        let message = humanize_git_failure(
            &["clone", "--quiet", "--", "/tmp/team", "/tmp/checkout"],
            "Schwerwiegend: kein Git-Repository",
        );

        assert!(message.summary.as_deref().is_some_and(|summary| {
            summary.contains("Could not clone local repository `/tmp/team`")
        }));
        assert!(
            message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("path is readable"))
        );
        assert!(
            !message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("network/proxy"))
        );
    }

    #[test]
    fn local_clone_preflight_should_report_missing_and_non_repository_paths() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let missing = temp_dir.path().join("missing");
        let error = preflight_local_clone_source(&missing.to_string_lossy(), temp_dir.path())
            .expect_err("missing local source should fail preflight");
        assert_eq!(
            error.to_string(),
            format!("local source path `{}` does not exist", missing.display())
        );

        let plain_directory = temp_dir.path().join("plain");
        fs::create_dir(&plain_directory).expect("plain directory should be created");
        let error =
            preflight_local_clone_source(&plain_directory.to_string_lossy(), temp_dir.path())
                .expect_err("non-repository local source should fail preflight");
        assert_eq!(
            error.to_string(),
            format!(
                "local source path `{}` is not a Git repository (missing .git)",
                plain_directory.display()
            )
        );
    }

    #[test]
    fn local_clone_preflight_should_allow_git_bundle_files() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let source = temp_dir.path().join("source");
        fs::create_dir(&source).expect("source directory should be created");
        init_repo(&source).expect("source repository should initialize");
        fs::write(source.join("README.md"), "# Bundle\n")
            .expect("bundle content should be written");
        run_git(&source, &["add", "README.md"]).expect("bundle content should be staged");
        run_git(
            &source,
            &[
                "-c",
                "user.name=Dalo Test",
                "-c",
                "user.email=dalo@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-qm",
                "bundle fixture",
            ],
        )
        .expect("bundle source should commit");
        let bundle = temp_dir.path().join("source.bundle");
        let bundle_arg = bundle.to_string_lossy().into_owned();
        run_git(&source, &["bundle", "create", &bundle_arg, "--all"])
            .expect("bundle should be created");

        preflight_local_clone_source(&bundle_arg, temp_dir.path())
            .expect("valid Git bundle should pass local preflight");
        clone_repo(&bundle_arg, &temp_dir.path().join("clone"))
            .expect("valid Git bundle should remain cloneable");
    }

    #[test]
    fn local_clone_preflight_should_escape_control_characters() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let missing = temp_dir.path().join("missing\nrepo");
        let error = preflight_local_clone_source(&missing.to_string_lossy(), temp_dir.path())
            .expect_err("missing local source should fail preflight");
        let message = error.to_string();

        assert!(message.contains("missing\\nrepo"));
        assert!(!message.contains("missing\nrepo"));
    }

    #[test]
    fn humanize_git_failure_should_redact_url_userinfo() {
        let secret_url = "https://octo:token-value@example.invalid/repo.git";
        let message = humanize_git_failure(
            &["clone", "--quiet", "--", secret_url, "/tmp/checkout"],
            &format!("fatal: unable to access '{secret_url}': denied"),
        );

        assert!(
            message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("https://***@example.invalid/repo.git"))
        );
        assert!(
            !message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("token-value"))
        );
        assert!(!message.stderr.contains("token-value"));
    }

    #[test]
    fn validate_remote_url_should_allow_supported_transports() {
        for location in [
            "/tmp/repo",
            "./repo",
            "https://example.invalid/repo.git",
            "ssh://git@example.invalid/repo.git",
            "git://example.invalid/repo.git",
            "file:///tmp/repo",
            "git@example.invalid:owner/repo.git",
            "example.invalid:owner/repo.git",
        ] {
            assert!(
                validate_remote_url(location).is_ok(),
                "expected `{location}` to be accepted"
            );
        }
    }

    #[test]
    fn validate_remote_url_should_reject_unsafe_transports_and_userinfo() {
        for location in [
            "ext::sh -c 'touch /tmp/dalo-rce'",
            "ext::sh",
            "http://example.invalid/repo.git",
            "git+ssh://example.invalid/repo.git",
            "rsync://example.invalid/repo.git",
            "https://octo:token-value@example.invalid/repo.git",
            "ssh://octo:token-value@example.invalid/repo.git",
            "git@example.invalid:owner:repo.git",
            "git@-example.invalid:owner/repo.git",
        ] {
            assert!(
                matches!(
                    validate_remote_url(location),
                    Err(DaloError::UnsafeRemoteUrl)
                ),
                "expected `{location}` to be rejected"
            );
        }
    }

    #[test]
    fn clone_repo_should_reject_unsafe_transport_before_spawning_git() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let destination = temp_dir.path().join("checkout");

        let error = clone_repo("ext::sh -c 'touch /tmp/dalo-rce'", &destination)
            .expect_err("unsafe transport should be rejected before cloning");

        assert!(matches!(error, DaloError::UnsafeRemoteUrl));
        assert!(!destination.exists());
    }

    #[test]
    fn is_dirty_should_ignore_untracked_files_but_report_tracked_changes() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("repo dir should be created");
        run_git(&repo, &["init", "-q"]).expect("repo should initialize");
        fs::write(repo.join("SKILL.md"), "# Skill\n").expect("skill should be written");
        run_git(&repo, &["add", "."]).expect("files should stage");
        run_git(
            &repo,
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
        )
        .expect("initial commit should succeed");

        // A stray untracked file must not count as dirty.
        fs::write(repo.join(".DS_Store"), b"junk").expect("untracked file should be written");
        assert!(!is_dirty(&repo).expect("status should run"));

        // A tracked modification must still count as dirty.
        fs::write(repo.join("SKILL.md"), "# Changed\n").expect("tracked file should change");
        assert!(is_dirty(&repo).expect("status should run"));
    }

    #[test]
    fn validate_manifest_revision_should_reject_revision_expressions() {
        for accepted in [
            "0123456789abcdef0123456789abcdef01234567",
            "v1.0.0",
            "main",
            "release/2024",
        ] {
            assert!(
                validate_manifest_revision(accepted).is_ok(),
                "expected `{accepted}` to be accepted"
            );
        }
        for rejected in [
            "", "-x", "a b", "main^", "HEAD~1", "a..b", "HEAD@{1}", "a:b", "a*",
        ] {
            assert!(
                validate_manifest_revision(rejected).is_err(),
                "expected `{rejected}` to be rejected"
            );
        }
    }

    #[test]
    fn humanize_git_failure_should_explain_tracking_refresh_errors() {
        let message =
            humanize_git_failure(&["pull", "--ff-only", "--quiet"], "fatal: not possible");

        assert!(
            message
                .summary
                .as_deref()
                .is_some_and(|summary| summary.contains("Could not refresh this source"))
        );
        assert_eq!(message.stderr, "fatal: not possible");
    }

    #[test]
    fn clone_repo_should_treat_dash_prefixed_url_as_repository_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let repo = temp_dir.path().join("--upload-pack=touch-owned");
        let destination = temp_dir.path().join("checkout");
        fs::create_dir_all(&repo).expect("repo dir should be created");
        run_git_program(
            "git",
            &repo,
            &["init", "-q", "--bare"],
            Duration::from_secs(5),
        )
        .expect("bare repo should be initialized");

        clone_repo(
            repo.file_name()
                .expect("repo should have file name")
                .to_str()
                .expect("repo name should be utf-8"),
            &destination,
        )
        .expect("dash-prefixed repo path should clone after -- separator");

        assert!(destination.join(".git").is_dir());
    }

    fn write_executable(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("script should be written");
        let mut permissions = fs::metadata(&path)
            .expect("script metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("script should be executable");
        path
    }
}
