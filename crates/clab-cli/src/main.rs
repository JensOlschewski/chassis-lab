use anyhow::Result;
use clap::{Parser, Subcommand};

mod check;
mod convert;
mod inspect;
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
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(
            if error
                .downcast_ref::<validate::svc::ToleranceFailure>()
                .is_some()
            {
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
    }
}
