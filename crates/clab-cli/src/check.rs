use std::path::PathBuf;

use anyhow::{Result, bail};
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
    if !config.simulation_hdf5.is_file() {
        bail!(
            "simulation HDF5 does not exist: {}",
            config.simulation_hdf5.display()
        );
    }
    if let Some(validation) = &config.validation {
        clab_core::svc_reference::load_svc_reference(&validation.reference).map_err(|error| {
            anyhow::anyhow!(
                "failed to load SVC reference '{}': {error}",
                validation.reference.display()
            )
        })?;
    }
    if let Some(vertical) = &config.vertical {
        for path in [&vertical.measurement_hdf5, &vertical.simulation_hdf5] {
            if !path.is_file() {
                bail!("vertical input does not exist: {}", path.display());
            }
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
