use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};

mod check;
mod convert;
mod inspect;
mod signals;
mod validate;

#[derive(Debug, Parser)]
#[command(name = "clab", version, about = "ChassisLab vehicle analysis CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a configured analysis domain.
    Validate(validate::ValidateArgs),
    /// Validate configuration and its referenced inputs.
    Check(check::CheckArgs),
    /// Inspect a supported input without writing output.
    Inspect(inspect::InspectArgs),
    /// Convert a supported input to HDF5.
    Convert(convert::ConvertArgs),
    /// List canonical signals or generate a TOML mapping template.
    Signals(signals::SignalsArgs),
    /// Generate shell completion scripts.
    Completion { shell: Shell },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(
            if error.downcast_ref::<validate::ToleranceFailure>().is_some() {
                1
            } else {
                2
            },
        );
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Validate(args) => validate::validate(args),
        Command::Check(args) => check::check(args),
        Command::Inspect(args) => inspect::inspect(args),
        Command::Convert(args) => convert::convert(args),
        Command::Signals(args) => signals::signals(args),
        Command::Completion { shell } => {
            generate(shell, &mut Cli::command(), "clab", &mut std::io::stdout());
            Ok(())
        }
    }
}
