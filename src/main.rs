use std::process::ExitCode;

use dalo::cli::{Cli, run_cli};
use dalo::error::DaloExitCode;

fn main() -> ExitCode {
    match run_cli(Cli::parse_args()) {
        Ok(()) => DaloExitCode::Success.into(),
        Err(error) => {
            eprintln!("error: {error}");
            error.exit_code().into()
        }
    }
}
