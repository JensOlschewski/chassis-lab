//! Version 1 canonical HDF5 input schema.
//!
//! Root attributes: `schema_version` (uint32), `source_kind` (`simulation` or
//! `measurement`), `source_file`, `sample_count` (uint64), and `signal_count`
//! (uint64). `/axis/values` is uint64 sample indices for simulations and float64
//! physical time for measurements; its `kind` and `unit` attributes describe
//! which. Every signal lives at `/signals/<safe-name>` with
//! a one-dimensional float64 `values` dataset and `original_name` and `unit`
//! attributes. `/simulation` and `/measurement` retain source-specific data.

use std::path::{Path, PathBuf};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Simulation,
    Measurement,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Simulation => "simulation",
            Self::Measurement => "measurement",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    pub schema_version: u32,
    pub source_kind: SourceKind,
    pub source_file: String,
    pub sample_count: usize,
    pub signal_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalInfo {
    pub safe_name: String,
    pub original_name: String,
    pub unit: String,
    pub description: String,
    pub source_path: String,
    pub entity_name: String,
    pub component: String,
    pub adams_id: Option<u64>,
    pub object_id: Option<u64>,
    pub offset: Option<f64>,
    pub column: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    pub info: SignalInfo,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestInfo {
    pub safe_name: String,
    pub name: String,
    pub description: String,
    pub data_type: String,
    pub unit: String,
    pub value: String,
}

#[derive(Debug, Error)]
pub enum CanonicalHdf5Error {
    #[error(transparent)]
    Hdf5(#[from] hdf5::Error),
    #[error("{0}")]
    Format(String),
    #[error("signal '{0}' was not found")]
    NotFound(String),
    #[error("signal '{0}' is ambiguous: {1}")]
    Ambiguous(String, String),
    #[error("signal '{0}' has unit '{1}', expected '{2}'")]
    Unit(String, String, String),
}

#[derive(Debug)]
pub struct CanonicalFile {
    file: File,
    summary: Summary,
    signals: Vec<SignalInfo>,
    test_info: Vec<TestInfo>,
}

impl CanonicalFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CanonicalHdf5Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let (summary, signals, test_info) = validate(&path, &file)?;
        Ok(Self {
            file,
            summary,
            signals,
            test_info,
        })
    }
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
    pub fn signals(&self) -> &[SignalInfo] {
        &self.signals
    }
    pub fn test_info(&self) -> &[TestInfo] {
        &self.test_info
    }
    pub fn read_axis(&self) -> Result<Vec<f64>, CanonicalHdf5Error> {
        match self.summary.source_kind {
            SourceKind::Simulation => Ok(self
                .file
                .dataset("axis/values")?
                .read_raw::<u64>()?
                .into_iter()
                .map(|v| v as f64)
                .collect()),
            SourceKind::Measurement => Ok(self.file.dataset("axis/values")?.read_raw::<f64>()?),
        }
    }
    pub fn matching_signals(&self, text: &str) -> Vec<&SignalInfo> {
        let text = text.to_lowercase();
        self.signals
            .iter()
            .filter(|info| {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    info.safe_name,
                    info.original_name,
                    info.source_path,
                    info.component,
                    info.description,
                    info.unit
                )
                .to_lowercase()
                .contains(&text)
            })
            .collect()
    }
    pub fn resolve_signal(
        &self,
        name: &str,
        component: Option<&str>,
    ) -> Result<&SignalInfo, CanonicalHdf5Error> {
        let name = name.strip_prefix("/signals/").unwrap_or(name);
        let matches: Vec<_> = self
            .signals
            .iter()
            .filter(|info| {
                let name_matches = name == info.safe_name
                    || name == info.original_name
                    || name == info.source_path
                    || name == info.entity_name;
                name_matches && component.is_none_or(|component| component == info.component)
            })
            .collect();
        match matches.as_slice() {
            [] => Err(CanonicalHdf5Error::NotFound(match component {
                Some(component) => format!("{name}/{component}"),
                None => name.into(),
            })),
            [info] => Ok(info),
            matches => Err(CanonicalHdf5Error::Ambiguous(
                name.into(),
                matches
                    .iter()
                    .map(|info| info.safe_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
        }
    }
    pub fn read_signal(
        &self,
        name: &str,
        component: Option<&str>,
        expected_unit: Option<&str>,
    ) -> Result<Signal, CanonicalHdf5Error> {
        let info = self.resolve_signal(name, component)?;
        if let Some(expected) = expected_unit
            && !info.unit.is_empty()
            && info.unit != expected
        {
            return Err(CanonicalHdf5Error::Unit(
                info.safe_name.clone(),
                info.unit.clone(),
                expected.into(),
            ));
        }
        Ok(Signal {
            info: info.clone(),
            values: self
                .file
                .dataset(&format!("signals/{}/values", info.safe_name))?
                .read_raw()?,
        })
    }
}

fn validate(
    path: &Path,
    file: &File,
) -> Result<(Summary, Vec<SignalInfo>, Vec<TestInfo>), CanonicalHdf5Error> {
    let schema_version = u32_attr(file, "schema_version")?;
    if schema_version != SCHEMA_VERSION {
        return format(
            "unsupported canonical HDF5 schema version {schema_version}; expected {SCHEMA_VERSION}"
                .into(),
        );
    }
    let source_kind = match string(file, "source_kind")?.as_str() {
        "simulation" => SourceKind::Simulation,
        "measurement" => SourceKind::Measurement,
        value => {
            return format(format!(
                "source_kind must be 'simulation' or 'measurement', got '{value}'"
            ));
        }
    };
    let source_file = string(file, "source_file")?;
    if source_file.is_empty() {
        return format("source_file must not be empty".into());
    }
    let sample_count = count(u64_attr(file, "sample_count")?, "sample_count")?;
    let signal_count = count(u64_attr(file, "signal_count")?, "signal_count")?;
    let axis = file
        .group("axis")
        .map_err(|_| CanonicalHdf5Error::Format("missing HDF5 group: /axis".into()))?;
    match source_kind {
        SourceKind::Simulation => {
            axis_metadata(&axis, "sample_index", Some(""))?;
            values(
                &dataset(&axis, "values", "/axis")?,
                sample_count,
                "uint64",
                true,
                "/axis/values",
            )?;
        }
        SourceKind::Measurement => {
            axis_metadata(&axis, "time", None)?;
            values(
                &dataset(&axis, "values", "/axis")?,
                sample_count,
                "float64",
                false,
                "/axis/values",
            )?;
        }
    }
    let signals_group = file
        .group("signals")
        .map_err(|_| CanonicalHdf5Error::Format("missing HDF5 group: /signals".into()))?;
    let mut signals = Vec::new();
    for safe_name in signals_group.member_names()? {
        safe(&safe_name, "/signals")?;
        let signal = group(&signals_group, &safe_name, "/signals")?;
        values(
            &dataset(&signal, "values", &format!("/signals/{safe_name}"))?,
            sample_count,
            "float64",
            false,
            &format!("/signals/{safe_name}/values"),
        )?;
        let info = SignalInfo {
            safe_name: safe_name.clone(),
            original_name: string(&signal, "original_name")?,
            unit: string(&signal, "unit")?,
            description: optional_string(&signal, "description")?.unwrap_or_default(),
            source_path: optional_string(&signal, "source_path")?.unwrap_or_default(),
            entity_name: optional_string(&signal, "entity_name")?.unwrap_or_default(),
            component: optional_string(&signal, "component")?.unwrap_or_default(),
            adams_id: optional_u64(&signal, "adams_id")?,
            object_id: optional_u64(&signal, "object_id")?,
            offset: optional_f64(&signal, "offset")?,
            column: optional_u64(&signal, "column")?,
        };
        if info.offset.is_some_and(|value| !value.is_finite()) {
            return format(format!("/signals/{safe_name}/offset must be finite"));
        }
        signals.push(info);
    }
    if signals.len() != signal_count {
        return format(format!(
            "signal_count is {signal_count}, but /signals contains {} signals",
            signals.len()
        ));
    }
    let test_info =
        if source_kind == SourceKind::Measurement
            && file
                .member_names()?
                .iter()
                .any(|name| name == "measurement")
        {
            test_info(&file.group("measurement").map_err(|_| {
                CanonicalHdf5Error::Format("missing HDF5 group: /measurement".into())
            })?)?
        } else {
            Vec::new()
        };
    Ok((
        Summary {
            path: path.into(),
            schema_version,
            source_kind,
            source_file,
            sample_count,
            signal_count,
        },
        signals,
        test_info,
    ))
}

fn test_info(measurement: &Group) -> Result<Vec<TestInfo>, CanonicalHdf5Error> {
    if !measurement
        .member_names()?
        .iter()
        .any(|name| name == "test_info")
    {
        return Ok(Vec::new());
    }
    let entries = group(measurement, "test_info", "/measurement/test_info")?;
    entries
        .member_names()?
        .into_iter()
        .map(|safe_name| {
            safe(&safe_name, "/measurement/test_info")?;
            let entry = group(&entries, &safe_name, "/measurement/test_info")?;
            Ok(TestInfo {
                safe_name,
                name: string(&entry, "name")?,
                description: string(&entry, "description")?,
                data_type: string(&entry, "data_type")?,
                unit: string(&entry, "unit")?,
                value: string(&entry, "value")?,
            })
        })
        .collect()
}

fn values(
    dataset: &hdf5::Dataset,
    count: usize,
    kind: &str,
    integer: bool,
    path: &str,
) -> Result<(), CanonicalHdf5Error> {
    if (integer && !dataset.dtype()?.is::<u64>()) || (!integer && !dataset.dtype()?.is::<f64>()) {
        return format(format!("{path} must have dtype {kind}"));
    }
    if dataset.ndim() != 1 || dataset.shape()[0] != count {
        return format(format!(
            "{path} must be one-dimensional with {count} entries"
        ));
    }
    Ok(())
}
fn axis_metadata(
    axis: &Group,
    expected_kind: &str,
    expected_unit: Option<&str>,
) -> Result<(), CanonicalHdf5Error> {
    let kind = string(axis, "kind")?;
    if kind != expected_kind {
        return format(format!(
            "/axis/kind must be '{expected_kind}', got '{kind}'"
        ));
    }
    let unit = string(axis, "unit")?;
    if let Some(expected_unit) = expected_unit
        && unit != expected_unit
    {
        return format(format!(
            "/axis/unit must be '{expected_unit}', got '{unit}'"
        ));
    }
    Ok(())
}
fn group(parent: &Group, name: &str, path: &str) -> Result<Group, CanonicalHdf5Error> {
    parent
        .group(name)
        .map_err(|_| CanonicalHdf5Error::Format(format!("missing HDF5 group: {path}")))
}
fn dataset(parent: &Group, name: &str, path: &str) -> Result<hdf5::Dataset, CanonicalHdf5Error> {
    parent.dataset(name).map_err(|_| {
        CanonicalHdf5Error::Format(format!("{path}/{name} is missing or not a dataset"))
    })
}
fn safe(name: &str, parent: &str) -> Result<(), CanonicalHdf5Error> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        format(format!("{parent}/{name} is not a safe group name"))
    } else {
        Ok(())
    }
}
fn count(value: u64, name: &str) -> Result<usize, CanonicalHdf5Error> {
    usize::try_from(value)
        .map_err(|_| CanonicalHdf5Error::Format(format!("{name} exceeds platform size")))
}
fn u32_attr(location: &Location, name: &str) -> Result<u32, CanonicalHdf5Error> {
    location
        .attr(name)
        .map_err(|_| CanonicalHdf5Error::Format(format!("missing HDF5 attribute: {name}")))?
        .read_scalar()
        .map_err(|_| CanonicalHdf5Error::Format(format!("HDF5 attribute {name} must be uint32")))
}
fn u64_attr(location: &Location, name: &str) -> Result<u64, CanonicalHdf5Error> {
    location
        .attr(name)
        .map_err(|_| CanonicalHdf5Error::Format(format!("missing HDF5 attribute: {name}")))?
        .read_scalar()
        .map_err(|_| CanonicalHdf5Error::Format(format!("HDF5 attribute {name} must be uint64")))
}
fn optional_u64(location: &Location, name: &str) -> Result<Option<u64>, CanonicalHdf5Error> {
    if location.attr_names()?.iter().any(|value| value == name) {
        location.attr(name)?.read_scalar().map(Some).map_err(|_| {
            CanonicalHdf5Error::Format(format!("HDF5 attribute {name} must be uint64"))
        })
    } else {
        Ok(None)
    }
}
fn optional_f64(location: &Location, name: &str) -> Result<Option<f64>, CanonicalHdf5Error> {
    if location.attr_names()?.iter().any(|value| value == name) {
        location.attr(name)?.read_scalar().map(Some).map_err(|_| {
            CanonicalHdf5Error::Format(format!("HDF5 attribute {name} must be float64"))
        })
    } else {
        Ok(None)
    }
}
fn string(location: &Location, name: &str) -> Result<String, CanonicalHdf5Error> {
    optional_string(location, name)?
        .ok_or_else(|| CanonicalHdf5Error::Format(format!("missing HDF5 attribute: {name}")))
}
fn optional_string(location: &Location, name: &str) -> Result<Option<String>, CanonicalHdf5Error> {
    if !location.attr_names()?.iter().any(|value| value == name) {
        return Ok(None);
    }
    location
        .attr(name)?
        .read_scalar::<VarLenUnicode>()
        .map(|value| Some(value.as_str().into()))
        .map_err(|_| {
            CanonicalHdf5Error::Format(format!(
                "HDF5 attribute {name} must be a variable-length UTF-8 string"
            ))
        })
}
fn format<T>(message: String) -> Result<T, CanonicalHdf5Error> {
    Err(CanonicalHdf5Error::Format(message))
}
