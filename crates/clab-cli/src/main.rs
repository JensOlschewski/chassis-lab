use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

mod analysis;
mod analyze;
mod compare;
mod convert;
mod inspect;
mod report;
mod signals;

#[derive(Debug, Parser)]
#[command(name = "clab", version, about = "ChassisLab vehicle analysis CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Analyze normalized input or run manifest.
    Analyze(analyze::AnalyzeArgs),
    /// Compare compatible SVC or KnC vertical inputs.
    Compare(compare::CompareArgs),
    /// Inspect normalized input or canonical mapping requirements.
    Inspect(inspect::InspectArgs),
    /// Convert a supported input to HDF5.
    Convert(convert::ConvertArgs),
    /// Generate shell completion scripts.
    Completion { shell: Shell },
}

#[cfg(windows)]
fn bundled_python_home() -> Option<std::path::PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let python_home = executable.parent()?.parent()?.join("lib/python3.12");
    python_home.is_dir().then_some(python_home)
}

fn main() -> ExitCode {
    // Embedded CPython must not inherit host interpreter configuration.
    unsafe {
        std::env::remove_var("PYTHONHOME");
        std::env::remove_var("PYTHONPATH");
        #[cfg(windows)]
        if let Some(python_home) = bundled_python_home() {
            std::env::set_var("PYTHONHOME", python_home);
        }
    }

    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.downcast_ref::<compare::ToleranceFailure>().is_some() => {
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Analyze(args) => analyze::analyze(args),
        Command::Compare(args) => compare::compare(args),
        Command::Inspect(args) => inspect::inspect(args),
        Command::Convert(args) => convert::convert(args),
        Command::Completion { shell } => {
            generate(shell, &mut Cli::command(), "clab", &mut std::io::stdout());
            Ok(())
        }
    }
}
