use std::process::ExitCode;

use dalo::cli::{Cli, run_cli};
use dalo::error::{DaloError, DaloExitCode};
use dalo::term;
use serde::Serialize;

fn main() -> ExitCode {
    match run_cli(Cli::parse_args()) {
        Ok(()) => DaloExitCode::Success.into(),
        Err(error) => {
            let code = error.exit_code();
            if json_requested() {
                print_json_error(&error, code);
            } else {
                eprintln!("{}: {error}", term::error_label("error"));
            }
            code.into()
        }
    }
}

fn json_requested() -> bool {
    std::env::args_os()
        .skip(1)
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--json")
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

fn print_json_error(error: &DaloError, code: DaloExitCode) {
    let message = error.to_string();
    let payload = JsonError {
        error: JsonErrorBody {
            code: code.as_str(),
            message: &message,
        },
    };
    if serde_json::to_writer_pretty(std::io::stderr(), &payload).is_ok() {
        eprintln!();
    } else {
        eprintln!("error: {message}");
    }
}
