//! Read-only dispatch for supported HDF5 input formats.

use std::path::Path;

use hdf5::File;
use hdf5::types::VarLenUnicode;
use thiserror::Error;

use crate::measurement_hdf5::{MeasurementFile, MeasurementHdf5Error};
use crate::simulation_hdf5::{SimulationFile, SimulationHdf5Error};

#[derive(Debug)]
pub enum Hdf5Input {
    Simulation(SimulationFile),
    Measurement(MeasurementFile),
}

#[derive(Debug, Error)]
pub enum Hdf5InputError {
    #[error("invalid HDF5 input: {0}")]
    Hdf5(#[from] hdf5::Error),
    #[error("HDF5 attribute source_kind must be a variable-length UTF-8 string")]
    InvalidSourceKind,
    #[error("unsupported HDF5 source_kind '{0}'; expected 'measurement'")]
    UnsupportedSourceKind(String),
    #[error(transparent)]
    Simulation(#[from] SimulationHdf5Error),
    #[error(transparent)]
    Measurement(#[from] MeasurementHdf5Error),
}

pub fn open_hdf5_input(path: impl AsRef<Path>) -> Result<Hdf5Input, Hdf5InputError> {
    let path = path.as_ref();
    let file = File::open(path)?;
    let source_kind = if file.attr_names()?.iter().any(|name| name == "source_kind") {
        Some(
            file.attr("source_kind")?
                .read_scalar::<VarLenUnicode>()
                .map_err(|_| Hdf5InputError::InvalidSourceKind)?
                .as_str()
                .to_owned(),
        )
    } else {
        None
    };
    drop(file);

    match source_kind.as_deref() {
        None => Ok(Hdf5Input::Simulation(SimulationFile::open(path)?)),
        Some("measurement") => Ok(Hdf5Input::Measurement(MeasurementFile::open(path)?)),
        Some(value) => Err(Hdf5InputError::UnsupportedSourceKind(value.into())),
    }
}
