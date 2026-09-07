use std::path::Path;

use anyhow::{Context, Result, bail};

pub mod acd;
pub mod hdf5_writer;
pub mod measurement_hdf5;
pub mod model;
pub mod parser;

/// Converts an Adams RES result to simulation HDF5 without leaving a partial output on failure.
pub fn convert_res(source: &Path, output: &Path) -> Result<()> {
    let result = parser::parse_file(source)
        .with_context(|| format!("failed to parse RES file '{}'", source.display()))?;
    write_atomically(output, |temporary| {
        hdf5_writer::write_file(temporary, source, &result)
    })
}

pub(crate) fn write_atomically(
    output: &Path,
    write: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let temporary = temporary_path(output)?;
    if let Err(error) = write(&temporary) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary, output) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "failed to replace output '{}' with completed HDF5 file",
                output.display()
            )
        });
    }
    Ok(())
}

fn temporary_path(output: &Path) -> Result<std::path::PathBuf> {
    for attempt in 0..1_000 {
        let mut name = output.as_os_str().to_os_string();
        name.push(format!(".tmp-{}-{attempt}", std::process::id()));
        let temporary = std::path::PathBuf::from(name);
        if !temporary.exists() {
            return Ok(temporary);
        }
    }
    bail!(
        "could not allocate temporary output alongside '{}'",
        output.display()
    )
}
