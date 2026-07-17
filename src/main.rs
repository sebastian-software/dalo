use std::process::ExitCode;

use dalo::cli::{Cli, run_cli};
use dalo::error::{DaloError, DaloExitCode};
use dalo::term;
use serde::Serialize;

fn main() -> ExitCode {
    let cli = Cli::parse_args();
    let json = cli.json;
    match run_cli(cli) {
        Ok(()) => DaloExitCode::Success.into(),
        Err(error) => {
            let code = error.exit_code();
            if json {
                print_json_error(&error, code);
            } else {
                eprintln!("{}: {error}", term::error_label("error"));
            }
            code.into()
        }
    }
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
