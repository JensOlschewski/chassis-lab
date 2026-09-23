//! Strict read-only access to version 3 simulation result files.

use std::path::{Path, PathBuf};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use thiserror::Error;

pub const FORMAT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationSummary {
    pub path: PathBuf,
    pub format_version: u32,
    pub sample_count: usize,
    pub signal_count: usize,
    pub entity_count: usize,
    pub source_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalInfo {
    pub group_name: String,
    pub original_name: String,
    pub source_path: String,
    pub component: String,
    pub unit: String,
    pub adams_id: u64,
    pub column: usize,
}

impl SignalInfo {
    pub fn entity(&self) -> &str {
        if !self.source_path.is_empty() {
            &self.source_path
        } else if !self.original_name.is_empty() {
            &self.original_name
        } else {
            &self.group_name
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub info: SignalInfo,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct Hdf5FormatError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct SignalNotFoundError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct AmbiguousSignalError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct SignalUnitError(pub String);

#[derive(Debug, Error)]
pub enum SimulationHdf5Error {
    #[error(transparent)]
    Hdf5(#[from] hdf5::Error),
    #[error(transparent)]
    Format(#[from] Hdf5FormatError),
    #[error(transparent)]
    NotFound(#[from] SignalNotFoundError),
    #[error(transparent)]
    Ambiguous(#[from] AmbiguousSignalError),
    #[error(transparent)]
    Unit(#[from] SignalUnitError),
}

#[derive(Debug)]
pub struct SimulationFile {
    file: File,
    summary: SimulationSummary,
    signals: Vec<SignalInfo>,
}

impl SimulationFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SimulationHdf5Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let (summary, signals) = validate_file(&path, &file)?;
        Ok(Self {
            file,
            summary,
            signals,
        })
    }

    pub fn summary(&self) -> &SimulationSummary {
        &self.summary
    }

    pub fn signals(&self) -> &[SignalInfo] {
        &self.signals
    }

    pub fn matching_signals(&self, text: &str) -> Vec<&SignalInfo> {
        let query = text.to_lowercase();
        self.signals
            .iter()
            .filter(|info| {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    info.group_name,
                    info.original_name,
                    info.source_path,
                    info.component,
                    info.unit,
                    info.adams_id
                )
                .to_lowercase()
                .contains(&query)
            })
            .collect()
    }

    pub fn resolve_signal(
        &self,
        entity: &str,
        component: &str,
    ) -> Result<&SignalInfo, SimulationHdf5Error> {
        let matches: Vec<_> = self
            .signals
            .iter()
            .filter(|info| {
                info.component == component
                    && if let Some(group_name) = entity.strip_prefix("/results/") {
                        info.group_name == group_name
                    } else {
                        entity == info.source_path
                            || entity == info.original_name
                            || (info.source_path.is_empty()
                                && info.original_name.is_empty()
                                && entity == info.group_name)
                    }
            })
            .collect();
        match matches.as_slice() {
            [] => {
                let available: Vec<_> = self
                    .signals
                    .iter()
                    .filter(|info| {
                        entity == format!("/results/{}", info.group_name)
                            || entity == info.source_path
                            || entity == info.original_name
                    })
                    .map(|info| info.component.as_str())
                    .collect();
                let detail = if available.is_empty() {
                    String::new()
                } else {
                    let mut available = available;
                    available.sort_unstable();
                    available.dedup();
                    format!("; available components: {}", available.join(", "))
                };
                Err(SimulationHdf5Error::NotFound(SignalNotFoundError(format!(
                    "signal '{entity}/{component}' was not found{detail}"
                ))))
            }
            [info] => Ok(info),
            matches => {
                let mut groups: Vec<_> = matches
                    .iter()
                    .map(|info| info.group_name.as_str())
                    .collect();
                groups.sort_unstable();
                Err(SimulationHdf5Error::Ambiguous(AmbiguousSignalError(
                    format!(
                        "signal '{entity}/{component}' is ambiguous across groups: {}",
                        groups.join(", ")
                    ),
                )))
            }
        }
    }

    pub fn read_signal(
        &self,
        entity: &str,
        component: &str,
        expected_unit: Option<&str>,
    ) -> Result<Signal, SimulationHdf5Error> {
        let info = self.resolve_signal(entity, component)?;
        if let Some(expected_unit) = expected_unit
            && !info.unit.is_empty()
            && info.unit != expected_unit
        {
            return Err(SimulationHdf5Error::Unit(SignalUnitError(format!(
                "signal '{entity}/{component}' has unit '{}', expected '{expected_unit}'",
                info.unit
            ))));
        }
        let values = self
            .file
            .dataset(&format!("results/{}/values", info.group_name))?
            .read_slice_1d::<f64, _>((.., info.column))?
            .to_vec();
        Ok(Signal {
            info: info.clone(),
            values,
        })
    }
}

pub fn inspect_hdf5(path: impl AsRef<Path>) -> Result<SimulationSummary, SimulationHdf5Error> {
    Ok(SimulationFile::open(path)?.summary().clone())
}

fn validate_file(
    path: &Path,
    file: &File,
) -> Result<(SimulationSummary, Vec<SignalInfo>), SimulationHdf5Error> {
    let format_version = required_u32(file, "format_version")?;
    if format_version != FORMAT_VERSION {
        return format_error(format!(
            "unsupported HDF5 format version {format_version}; expected {FORMAT_VERSION}"
        ));
    }
    let sample_count = usize::try_from(required_u64(file, "sample_count")?).map_err(|_| {
        SimulationHdf5Error::Format(Hdf5FormatError("sample_count exceeds platform size".into()))
    })?;
    let declared_signal_count =
        usize::try_from(required_u64(file, "signal_count")?).map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(
                "signal_count exceeds platform size".into(),
            ))
        })?;
    let source_file = required_string(file, "source_file")?;
    let results = file.group("results").map_err(|_| {
        SimulationHdf5Error::Format(Hdf5FormatError("missing HDF5 group: /results".into()))
    })?;
    let mut signals = Vec::new();
    let entity_names = results.member_names()?;
    for group_name in &entity_names {
        let entity = results.group(group_name).map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!(
                "/results/{group_name} is not a group"
            )))
        })?;
        signals.extend(validate_entity(group_name, &entity, sample_count)?);
    }
    if signals.len() != declared_signal_count {
        return format_error(format!(
            "signal_count is {declared_signal_count}, but entity datasets contain {} signals",
            signals.len()
        ));
    }
    Ok((
        SimulationSummary {
            path: path.to_path_buf(),
            format_version,
            sample_count,
            signal_count: signals.len(),
            entity_count: entity_names.len(),
            source_file,
        },
        signals,
    ))
}

fn validate_entity(
    group_name: &str,
    entity: &Group,
    sample_count: usize,
) -> Result<Vec<SignalInfo>, SimulationHdf5Error> {
    let prefix = format!("/results/{group_name}");
    let values = dataset(entity, "values", &prefix)?;
    let component_names = dataset(entity, "component_names", &prefix)?;
    let units = dataset(entity, "units", &prefix)?;
    let adams_ids = dataset(entity, "adams_ids", &prefix)?;
    if !values.dtype()?.is::<f64>() {
        return format_error(format!("{prefix}/values must have dtype float64"));
    }
    if !adams_ids.dtype()?.is::<u64>() {
        return format_error(format!("{prefix}/adams_ids must have dtype uint64"));
    }
    for (name, value) in [("component_names", &component_names), ("units", &units)] {
        if !value.dtype()?.is::<VarLenUnicode>() {
            return format_error(format!(
                "{prefix}/{name} must have dtype variable-length UTF-8 string"
            ));
        }
    }
    let shape = values.shape();
    if shape.len() != 2 {
        return format_error(format!("{prefix}/values must be two-dimensional"));
    }
    if shape[0] != sample_count {
        return format_error(format!(
            "{prefix}/values has {} samples; expected {sample_count}",
            shape[0]
        ));
    }
    let component_count = shape[1];
    for (name, value) in [
        ("component_names", &component_names),
        ("units", &units),
        ("adams_ids", &adams_ids),
    ] {
        if value.ndim() != 1 || value.shape()[0] != component_count {
            return format_error(format!(
                "{prefix}/{name} must contain {component_count} entries"
            ));
        }
    }
    let names = component_names.read_raw::<VarLenUnicode>()?;
    let units = units.read_raw::<VarLenUnicode>()?;
    let ids = adams_ids.read_raw::<u64>()?;
    let original_name = optional_string(entity, "original_name")?.unwrap_or_default();
    let source_path = optional_string(entity, "source_path")?.unwrap_or_default();
    Ok(names
        .into_iter()
        .zip(units)
        .zip(ids)
        .enumerate()
        .map(|(column, ((component, unit), adams_id))| SignalInfo {
            group_name: group_name.into(),
            original_name: original_name.clone(),
            source_path: source_path.clone(),
            component: component.as_str().into(),
            unit: unit.as_str().into(),
            adams_id,
            column,
        })
        .collect())
}

fn dataset(entity: &Group, name: &str, prefix: &str) -> Result<hdf5::Dataset, SimulationHdf5Error> {
    entity.dataset(name).map_err(|_| {
        SimulationHdf5Error::Format(Hdf5FormatError(format!(
            "{prefix}/{name} is missing or not a dataset"
        )))
    })
}

fn required_u32(location: &Location, name: &str) -> Result<u32, SimulationHdf5Error> {
    location
        .attr(name)
        .map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!("missing HDF5 attribute: {name}")))
        })?
        .read_scalar::<u32>()
        .map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!(
                "HDF5 attribute {name} must be uint32"
            )))
        })
}

fn required_u64(location: &Location, name: &str) -> Result<u64, SimulationHdf5Error> {
    location
        .attr(name)
        .map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!("missing HDF5 attribute: {name}")))
        })?
        .read_scalar::<u64>()
        .map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!(
                "HDF5 attribute {name} must be uint64"
            )))
        })
}

fn required_string(location: &Location, name: &str) -> Result<String, SimulationHdf5Error> {
    optional_string(location, name)?.ok_or_else(|| {
        SimulationHdf5Error::Format(Hdf5FormatError(format!("missing HDF5 attribute: {name}")))
    })
}

fn optional_string(location: &Location, name: &str) -> Result<Option<String>, SimulationHdf5Error> {
    if !location
        .attr_names()?
        .iter()
        .any(|attribute| attribute == name)
    {
        return Ok(None);
    }
    location
        .attr(name)?
        .read_scalar::<VarLenUnicode>()
        .map(|value| Some(value.as_str().into()))
        .map_err(|_| {
            SimulationHdf5Error::Format(Hdf5FormatError(format!(
                "HDF5 attribute {name} must be a variable-length UTF-8 string"
            )))
        })
}

fn format_error<T>(message: String) -> Result<T, SimulationHdf5Error> {
    Err(SimulationHdf5Error::Format(Hdf5FormatError(message)))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hdf5::types::VarLenUnicode;
    use hdf5::{File, Location};
    use tempfile::TempDir;

    use super::*;

    fn fixture() -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("result.h5");
        let file = File::create(&path).unwrap();
        file.new_attr::<u32>()
            .create("format_version")
            .unwrap()
            .write_scalar(&3)
            .unwrap();
        file.new_attr::<u64>()
            .create("sample_count")
            .unwrap()
            .write_scalar(&2)
            .unwrap();
        file.new_attr::<u64>()
            .create("signal_count")
            .unwrap()
            .write_scalar(&4)
            .unwrap();
        write_string(&file, "source_file", "result.res");
        let results = file.create_group("results").unwrap();
        entity(
            &results,
            "front",
            "Front wheel",
            "/vehicle/front",
            &["force", "travel"],
            &["newton", "mm"],
            &[1, 2],
            &[1.0, 2.0, 3.0, 4.0],
        );
        entity(
            &results,
            "rear",
            "Rear wheel",
            "/vehicle/rear",
            &["force", "travel"],
            &["newton", "mm"],
            &[3, 4],
            &[5.0, 6.0, 7.0, 8.0],
        );
        file.flush().unwrap();
        (directory, path)
    }

    #[allow(clippy::too_many_arguments)]
    fn entity(
        results: &Group,
        group_name: &str,
        original_name: &str,
        source_path: &str,
        names: &[&str],
        units: &[&str],
        ids: &[u64],
        values: &[f64],
    ) {
        let entity = results.create_group(group_name).unwrap();
        write_string(&entity, "original_name", original_name);
        write_string(&entity, "source_path", source_path);
        entity
            .new_dataset::<f64>()
            .shape((2, names.len()))
            .create("values")
            .unwrap()
            .write_raw(values)
            .unwrap();
        let names: Vec<_> = names
            .iter()
            .map(|value| VarLenUnicode::from_str(value).unwrap())
            .collect();
        let units: Vec<_> = units
            .iter()
            .map(|value| VarLenUnicode::from_str(value).unwrap())
            .collect();
        entity
            .new_dataset_builder()
            .with_data(&names)
            .create("component_names")
            .unwrap();
        entity
            .new_dataset_builder()
            .with_data(&units)
            .create("units")
            .unwrap();
        entity
            .new_dataset_builder()
            .with_data(ids)
            .create("adams_ids")
            .unwrap();
    }

    fn write_string(location: &Location, name: &str, value: &str) {
        location
            .new_attr::<VarLenUnicode>()
            .shape(())
            .create(name)
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str(value).unwrap())
            .unwrap();
    }

    #[test]
    fn reads_valid_file_and_resolves_source_group_and_original_names() {
        let (_directory, path) = fixture();
        let file = SimulationFile::open(&path).unwrap();
        assert_eq!(file.summary().signal_count, 4);
        assert_eq!(file.matching_signals("FRONT").len(), 2);
        assert_eq!(
            file.resolve_signal("/vehicle/front", "force")
                .unwrap()
                .adams_id,
            1
        );
        assert_eq!(
            file.resolve_signal("Front wheel", "travel").unwrap().column,
            1
        );
        assert_eq!(
            file.resolve_signal("/results/rear", "force")
                .unwrap()
                .adams_id,
            3
        );
        assert_eq!(
            file.read_signal("/vehicle/front", "travel", Some("mm"))
                .unwrap()
                .values,
            [2.0, 4.0]
        );
    }

    #[test]
    fn reports_lookup_and_unit_diagnostics() {
        let (_directory, path) = fixture();
        let writable = File::open_rw(&path).unwrap();
        writable
            .group("results/front")
            .unwrap()
            .attr("source_path")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("wheel").unwrap())
            .unwrap();
        writable
            .group("results/rear")
            .unwrap()
            .attr("source_path")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("wheel").unwrap())
            .unwrap();
        drop(writable);
        let file = SimulationFile::open(&path).unwrap();
        assert!(
            matches!(file.resolve_signal("/results/front", "missing"), Err(SimulationHdf5Error::NotFound(error)) if error.0.contains("available components: force, travel"))
        );
        assert!(matches!(
            file.resolve_signal("/vehicle/missing", "force"),
            Err(SimulationHdf5Error::NotFound(_))
        ));
        assert!(matches!(
            file.resolve_signal("wheel", "force"),
            Err(SimulationHdf5Error::Ambiguous(error)) if error.0.contains("front, rear")
        ));
        assert!(matches!(
            file.read_signal("wheel", "travel", Some("mm")),
            Err(SimulationHdf5Error::Ambiguous(_))
        ));
        assert!(matches!(
            file.read_signal("/results/front", "force", Some("mm")),
            Err(SimulationHdf5Error::Unit(_))
        ));
    }

    #[test]
    fn accepts_missing_unit_metadata_but_rejects_nonempty_mismatch() {
        let (_directory, path) = fixture();
        let writable = File::open_rw(&path).unwrap();
        let group = writable.group("results/front").unwrap();
        group.unlink("units").unwrap();
        let units = ["", "mm"]
            .into_iter()
            .map(|value| VarLenUnicode::from_str(value).unwrap())
            .collect::<Vec<_>>();
        group
            .new_dataset_builder()
            .with_data(&units)
            .create("units")
            .unwrap();
        drop(writable);

        let file = SimulationFile::open(&path).unwrap();
        assert!(
            file.read_signal("/vehicle/front", "force", Some("newton"))
                .is_ok()
        );
        assert!(
            file.read_signal("/vehicle/front", "force", Some("mm"))
                .is_ok()
        );
        assert!(matches!(
            file.read_signal("/vehicle/front", "travel", Some("newton")),
            Err(SimulationHdf5Error::Unit(_))
        ));
    }

    #[test]
    fn rejects_malformed_contracts() {
        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        file.attr("format_version")
            .unwrap()
            .write_scalar(&2_u32)
            .unwrap();
        drop(file);
        assert!(
            SimulationFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("unsupported HDF5 format version")
        );

        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        let group = file.group("results/front").unwrap();
        group.unlink("values").unwrap();
        group
            .new_dataset::<f32>()
            .shape((2, 2))
            .create("values")
            .unwrap()
            .write_raw(&[1.0_f32, 2.0, 3.0, 4.0])
            .unwrap();
        drop(file);
        assert!(
            SimulationFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("values must have dtype float64")
        );

        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        file.attr("signal_count")
            .unwrap()
            .write_scalar(&3_u64)
            .unwrap();
        drop(file);
        assert!(
            SimulationFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("signal_count is 3")
        );
    }
}
