use std::process::ExitCode;

use dalo::cli::{Cli, run_cli};
use dalo::error::DaloExitCode;
use dalo::store;
use dalo::term;
use serde::Serialize;

fn main() -> ExitCode {
    sigpipe::reset();
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
                eprintln!("{}: {message}", term::error_label("error"));
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
        eprintln!("error: {message}");
    }
}
