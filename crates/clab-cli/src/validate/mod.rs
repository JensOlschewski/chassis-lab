use anyhow::Result;
use clap::{Args, Subcommand};

pub(crate) mod svc;
pub(crate) mod vertical;

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    #[command(subcommand)]
    domain: ValidateDomain,
}

#[derive(Debug, Subcommand)]
enum ValidateDomain {
    /// Compare normalized SVC simulation signals with reviewed references.
    Svc(svc::SvcArgs),
    /// Compare normalized vertical measurement and simulation trajectories.
    Vertical(vertical::VerticalArgs),
}

pub(crate) fn validate(args: ValidateArgs) -> Result<()> {
    match args.domain {
        ValidateDomain::Svc(args) => svc::validate(args),
        ValidateDomain::Vertical(args) => vertical::validate(args),
    }
}
