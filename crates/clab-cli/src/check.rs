use std::path::PathBuf;

use anyhow::Result;
use clab_core::config::Domain;
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    /// TOML case configuration.
    config: PathBuf,
}

pub(crate) fn check(args: CheckArgs) -> Result<()> {
    let config = clab_core::config::load_config(&args.config).map_err(|error| {
        anyhow::anyhow!(
            "failed to load configuration '{}': {error}",
            args.config.display()
        )
    })?;
    for domain in config.configured_domains() {
        match domain {
            Domain::Svc => crate::validate::svc::preflight(&config)?,
            Domain::Knc => crate::validate::vertical::preflight(&config)?,
        }
    }
    println!("configuration is valid: {}", config.path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn parses_check_command() {
        let cli = Cli::try_parse_from(["clab", "check", "case.toml"]).unwrap();
        assert!(format!("{cli:?}").contains("Check"));
    }
}
