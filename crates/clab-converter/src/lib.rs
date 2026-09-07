use std::path::Path;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

pub mod acd;
pub mod hdf5_writer;
pub mod measurement_hdf5;
pub mod model;
pub mod parser;

/// Converts an Adams RES result to simulation HDF5 without leaving a partial output on failure.
pub fn convert_res(source: &Path, output: &Path, overwrite: bool) -> Result<()> {
    let result = parser::parse_file(source)
        .with_context(|| format!("failed to parse RES file '{}'", source.display()))?;
    write_atomically(output, overwrite, |temporary| {
        hdf5_writer::write_file(temporary, source, &result)
    })
}

pub(crate) fn write_atomically(
    output: &Path,
    overwrite: bool,
    write: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let temporary = NamedTempFile::new_in(parent).with_context(|| {
        format!(
            "failed to create temporary output near '{}'",
            output.display()
        )
    })?;
    let temporary_path = temporary.path().to_path_buf();
    write(&temporary_path)?;
    let persist = if overwrite {
        temporary.persist(output)
    } else {
        temporary.persist_noclobber(output)
    };
    persist.map(|_| ()).with_context(|| {
        format!(
            "failed to publish completed HDF5 file to '{}'",
            output.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::write_atomically;
    use std::fs;

    #[test]
    fn failed_write_preserves_output_and_removes_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("output.h5");
        fs::write(&output, "existing output").unwrap();

        let error =
            write_atomically(&output, true, |_| Err(anyhow::anyhow!("write failed"))).unwrap_err();

        assert!(error.to_string().contains("write failed"));
        assert_eq!(fs::read_to_string(&output).unwrap(), "existing output");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
