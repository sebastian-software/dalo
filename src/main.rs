use std::process::ExitCode;

use skillmgr::cli::{Cli, run_cli};
use skillmgr::error::SkillmgrExitCode;

fn main() -> ExitCode {
    match run_cli(Cli::parse_args()) {
        Ok(()) => SkillmgrExitCode::Success.into(),
        Err(error) => {
            eprintln!("error: {error}");
            error.exit_code().into()
        }
    }
}
