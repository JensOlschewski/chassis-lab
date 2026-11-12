use std::process::ExitCode;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

mod analysis;
mod analyze;
mod compare;
mod convert;
mod inspect;
mod license;
mod progress;
mod report;
mod signals;
mod solve;

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
    /// Solve a multibody model with the MATLAB solver.
    Solve(solve::SolveArgs),
    /// Manage the license of this installation.
    #[command(subcommand)]
    License(license::LicenseCommand),
    /// Generate shell completion scripts.
    Completion { shell: Shell },
    #[command(hide = true, name = "__render-knc")]
    RenderKnc {
        request: std::path::PathBuf,
        output: std::path::PathBuf,
    },
}

impl Command {
    fn needs_license(&self) -> bool {
        matches!(
            self,
            Self::Analyze(_)
                | Self::Compare(_)
                | Self::Inspect(_)
                | Self::Convert(_)
                | Self::Solve(_)
        )
    }
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
            if error
                .downcast_ref::<convert::ConfirmationRequired>()
                .is_some()
            {
                // Lets a GUI caller tell "ask the user" apart from a failure.
                return ExitCode::from(3);
            }
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.command.needs_license() {
        license::require()?;
    }
    match cli.command {
        Command::Analyze(args) => analyze::analyze(args),
        Command::Compare(args) => compare::compare(args),
        Command::Inspect(args) => inspect::inspect(args),
        Command::Convert(args) => convert::convert(args),
        Command::Solve(args) => solve::solve(args),
        Command::License(command) => license::run(command),
        Command::Completion { shell } => {
            generate(shell, &mut Cli::command(), "clab", &mut std::io::stdout());
            Ok(())
        }
        Command::RenderKnc { request, output } => analysis::knc::render_worker(&request, &output),
    }
}
