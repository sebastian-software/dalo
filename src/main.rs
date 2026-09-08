use std::process::ExitCode;

use dalo::cli::{Cli, run_cli};
use dalo::error::DaloExitCode;
use dalo::store;
use dalo::term;
use serde::Serialize;

fn main() -> ExitCode {
    sigpipe::reset();
    #[cfg(target_os = "linux")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("__delivery-sandbox")) {
        return match dalo::delivery::run_linux_delivery_sandbox(
            std::env::args_os().skip(2).collect(),
        ) {
            Ok(()) => DaloExitCode::Success.into(),
            Err(error) => {
                eprintln!("error: {error}");
                DaloExitCode::UnsafeState.into()
            }
        };
    }
    let cli = Cli::parse_args();
    let json = cli.json;
    let store_root = store::resolve_store_path(cli.store.as_deref()).ok();
    match run_cli(cli) {
        Ok(()) => DaloExitCode::Success.into(),
        Err(error) => {
            let code = error.exit_code();
            let message = store_root.as_ref().map_or_else(
                || error.to_string(),
                |store_root| store::contextualize_dalo_commands(store_root, &error.to_string()),
            );
            if json {
                print_json_error(&message, code);
            } else {
                let message = store_root.as_ref().map_or_else(
                    || dalo::status::compact_human_text(&message),
                    |store_root| dalo::status::compact_store_text(store_root, &message),
                );
                let terminal_message = terminal_safe_error_message(&message);
                eprintln!("{}: {}", term::error_label("error"), terminal_message);
            }
            code.into()
        }
    }
}

const GIT_SAID_PARAGRAPH: &str = "\n\nGit said: ";

fn terminal_safe_error_message(message: &str) -> String {
    if let Some((summary, git_stderr)) = message.split_once(GIT_SAID_PARAGRAPH)
        && is_trusted_git_failure_summary(summary)
    {
        return format!(
            "{}{}{}",
            term::terminal_safe_text(summary),
            GIT_SAID_PARAGRAPH,
            term::terminal_safe_text(git_stderr)
        );
    }
    term::terminal_safe_text(message)
}

fn is_trusted_git_failure_summary(message: &str) -> bool {
    let Some((_, summary)) = message.split_once(": ") else {
        return false;
    };

    matches!(
        summary,
        "Could not refresh this source. Check network/proxy access, repository permissions, and whether the tracking branch can fast-forward."
            | "Could not check the repository for upstream changes. Check network/proxy access and repository permissions."
    ) || (summary.starts_with("Could not clone repository `")
        && summary.ends_with("`. Check the URL, network/proxy access, and repository permissions."))
        || (summary.starts_with("Could not clone local repository `")
            && summary
                .ends_with("`. Check that the path is readable and points to a Git repository."))
}

#[derive(Serialize)]
struct JsonError<'a> {
    error: JsonErrorBody<'a>,
}

#[derive(Serialize)]
struct JsonErrorBody<'a> {
    code: &'static str,
    message: &'a str,
}

fn print_json_error(message: &str, code: DaloExitCode) {
    let payload = JsonError {
        error: JsonErrorBody {
            code: code.as_str(),
            message,
        },
    };
    if serde_json::to_writer_pretty(std::io::stderr(), &payload).is_ok() {
        eprintln!();
    } else {
        eprintln!("error: {}", terminal_safe_error_message(message));
    }
}

#[cfg(test)]
mod tests {
    use super::terminal_safe_error_message;

    #[test]
    fn terminal_safe_error_message_should_preserve_trusted_git_paragraphs() {
        let message = "command `git\u{1b}]0;program\u{7}` failed with status 128: Could not clone repository `team\u{7}`. Check the URL, network/proxy access, and repository permissions.\n\nGit said: fatal: \u{1b}[2Jbad\noutput";

        assert_eq!(
            terminal_safe_error_message(message),
            "command `git\\u{1b}]0;program\\u{7}` failed with status 128: Could not clone repository `team\\u{7}`. Check the URL, network/proxy access, and repository permissions.\n\nGit said: fatal: \\u{1b}[2Jbad\\noutput"
        );
    }
}
