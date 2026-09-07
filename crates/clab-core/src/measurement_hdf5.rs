//! Strict read-only access to version 1 measurement HDF5 files.
//!
//! Measurement files are distinct from simulation result files. Version 1 requires root
//! attributes `format_version` (`uint32`, value 1), `source_kind` (UTF-8, `measurement`),
//! `source_file` (nonempty UTF-8), `sample_count` (`uint64`), and `channel_count` (`uint64`).
//! Optional root `encoding` and `dialect` attributes are UTF-8. `/measurements/sample_axis` is
//! a one-dimensional `float64` dataset with `sample_count` entries. Each direct child of
//! `/measurements/channels` is a safe group name (ASCII letters, digits, `_`, or `-`) containing
//! a one-dimensional `float64` `values` dataset and required attributes: nonempty UTF-8
//! `original_name`, UTF-8 `description` and `unit`, finite `float64` `offset`, and `uint64`
//! `column`. Optional `/measurements/test_info` has safe group children whose `name`,
//! `description`, `data_type`, `unit`, and `value` attributes are UTF-8 strings.
//!
//! Sample-axis and channel values may contain NaN or infinity. Only structural numeric metadata,
//! currently channel `offset`, must be finite.

use std::path::{Path, PathBuf};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use thiserror::Error;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasurementSummary {
    pub path: PathBuf,
    pub format_version: u32,
    pub source_file: String,
    pub sample_count: usize,
    pub channel_count: usize,
    pub encoding: Option<String>,
    pub dialect: Option<String>,
    pub test_info_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelInfo {
    pub group_name: String,
    pub original_name: String,
    pub description: String,
    pub unit: String,
    pub offset: f64,
    pub column: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementChannel {
    pub info: ChannelInfo,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestInfo {
    pub group_name: String,
    pub name: String,
    pub description: String,
    pub data_type: String,
    pub unit: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct Hdf5FormatError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ChannelNotFoundError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct AmbiguousChannelError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ChannelUnitError(pub String);

#[derive(Debug, Error)]
pub enum MeasurementHdf5Error {
    #[error(transparent)]
    Hdf5(#[from] hdf5::Error),
    #[error(transparent)]
    Format(#[from] Hdf5FormatError),
    #[error(transparent)]
    NotFound(#[from] ChannelNotFoundError),
    #[error(transparent)]
    Ambiguous(#[from] AmbiguousChannelError),
    #[error(transparent)]
    Unit(#[from] ChannelUnitError),
}

#[derive(Debug)]
pub struct MeasurementFile {
    file: File,
    summary: MeasurementSummary,
    channels: Vec<ChannelInfo>,
    test_info: Vec<TestInfo>,
}

impl MeasurementFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MeasurementHdf5Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let (summary, channels, test_info) = validate_file(&path, &file)?;
        Ok(Self {
            file,
            summary,
            channels,
            test_info,
        })
    }

    pub fn summary(&self) -> &MeasurementSummary {
        &self.summary
    }

    pub fn channels(&self) -> &[ChannelInfo] {
        &self.channels
    }

    pub fn matching_channels(&self, text: &str) -> Vec<&ChannelInfo> {
        let query = text.to_lowercase();
        self.channels
            .iter()
            .filter(|info| {
                format!(
                    "{}\t{}\t{}\t{}\t{}",
                    info.group_name, info.original_name, info.description, info.unit, info.column
                )
                .to_lowercase()
                .contains(&query)
            })
            .collect()
    }

    pub fn resolve_channel(&self, name: &str) -> Result<&ChannelInfo, MeasurementHdf5Error> {
        let group_name = name.strip_prefix("/measurements/channels/");
        let matches: Vec<_> = self
            .channels
            .iter()
            .filter(|info| {
                group_name.is_some_and(|value| value == info.group_name)
                    || name == info.group_name
                    || name == info.original_name
            })
            .collect();
        match matches.as_slice() {
            [] => Err(MeasurementHdf5Error::NotFound(ChannelNotFoundError(
                format!("measurement channel '{name}' was not found"),
            ))),
            [info] => Ok(info),
            matches => {
                let mut groups: Vec<_> = matches
                    .iter()
                    .map(|info| info.group_name.as_str())
                    .collect();
                groups.sort_unstable();
                Err(MeasurementHdf5Error::Ambiguous(AmbiguousChannelError(
                    format!(
                        "measurement channel '{name}' is ambiguous across groups: {}",
                        groups.join(", ")
                    ),
                )))
            }
        }
    }

    pub fn read_sample_axis(&self) -> Result<Vec<f64>, MeasurementHdf5Error> {
        Ok(self
            .file
            .dataset("measurements/sample_axis")?
            .read_raw::<f64>()?)
    }

    pub fn read_channel(
        &self,
        name: &str,
        expected_unit: Option<&str>,
    ) -> Result<MeasurementChannel, MeasurementHdf5Error> {
        let info = self.resolve_channel(name)?;
        if let Some(expected_unit) = expected_unit
            && info.unit != expected_unit
        {
            return Err(MeasurementHdf5Error::Unit(ChannelUnitError(format!(
                "measurement channel '{name}' has unit '{}', expected '{expected_unit}'",
                info.unit
            ))));
        }
        let values = self
            .file
            .dataset(&format!("measurements/channels/{}/values", info.group_name))?
            .read_raw::<f64>()?;
        Ok(MeasurementChannel {
            info: info.clone(),
            values,
        })
    }

    pub fn test_info(&self) -> &[TestInfo] {
        &self.test_info
    }

    pub fn named_test_info(&self, name: &str) -> Result<&TestInfo, MeasurementHdf5Error> {
        let group_name = name.strip_prefix("/measurements/test_info/");
        let matches: Vec<_> = self
            .test_info
            .iter()
            .filter(|info| {
                group_name.is_some_and(|value| value == info.group_name)
                    || name == info.group_name
                    || name == info.name
            })
            .collect();
        match matches.as_slice() {
            [] => Err(MeasurementHdf5Error::NotFound(ChannelNotFoundError(
                format!("test metadata '{name}' was not found"),
            ))),
            [info] => Ok(info),
            matches => {
                let mut groups: Vec<_> = matches
                    .iter()
                    .map(|info| info.group_name.as_str())
                    .collect();
                groups.sort_unstable();
                Err(MeasurementHdf5Error::Ambiguous(AmbiguousChannelError(
                    format!(
                        "test metadata '{name}' is ambiguous across groups: {}",
                        groups.join(", ")
                    ),
                )))
            }
        }
    }
}

pub fn inspect_hdf5(path: impl AsRef<Path>) -> Result<MeasurementSummary, MeasurementHdf5Error> {
    Ok(MeasurementFile::open(path)?.summary().clone())
}

fn validate_file(
    path: &Path,
    file: &File,
) -> Result<(MeasurementSummary, Vec<ChannelInfo>, Vec<TestInfo>), MeasurementHdf5Error> {
    let format_version = required_u32(file, "format_version")?;
    if format_version != FORMAT_VERSION {
        return format_error(format!(
            "unsupported measurement HDF5 format version {format_version}; expected {FORMAT_VERSION}"
        ));
    }
    let source_kind = required_string(file, "source_kind")?;
    if source_kind != "measurement" {
        return format_error(format!(
            "HDF5 attribute source_kind must be 'measurement', got '{source_kind}'"
        ));
    }
    let source_file = required_nonempty_string(file, "source_file")?;
    let sample_count = platform_count(required_u64(file, "sample_count")?, "sample_count")?;
    let declared_channel_count =
        platform_count(required_u64(file, "channel_count")?, "channel_count")?;
    let encoding = optional_string(file, "encoding")?;
    let dialect = optional_string(file, "dialect")?;
    let measurements = required_group(file, "measurements", "/measurements")?;
    let sample_axis = dataset(&measurements, "sample_axis", "/measurements")?;
    validate_values(&sample_axis, sample_count, "/measurements/sample_axis")?;
    let channels_group = required_group(&measurements, "channels", "/measurements/channels")?;
    let mut channels = Vec::new();
    for group_name in channels_group.member_names()? {
        validate_safe_group_name(&group_name, "/measurements/channels")?;
        let channel = required_group(&channels_group, &group_name, "/measurements/channels")?;
        channels.push(validate_channel(&group_name, &channel, sample_count)?);
    }
    if channels.len() != declared_channel_count {
        return format_error(format!(
            "channel_count is {declared_channel_count}, but channel groups contain {} channels",
            channels.len()
        ));
    }
    let test_info = if measurements
        .member_names()?
        .iter()
        .any(|name| name == "test_info")
    {
        validate_test_info(&required_group(
            &measurements,
            "test_info",
            "/measurements/test_info",
        )?)?
    } else {
        Vec::new()
    };
    Ok((
        MeasurementSummary {
            path: path.to_path_buf(),
            format_version,
            source_file,
            sample_count,
            channel_count: channels.len(),
            encoding,
            dialect,
            test_info_count: test_info.len(),
        },
        channels,
        test_info,
    ))
}

fn validate_channel(
    group_name: &str,
    channel: &Group,
    sample_count: usize,
) -> Result<ChannelInfo, MeasurementHdf5Error> {
    let prefix = format!("/measurements/channels/{group_name}");
    validate_values(
        &dataset(channel, "values", &prefix)?,
        sample_count,
        &format!("{prefix}/values"),
    )?;
    let offset = required_f64(channel, "offset")?;
    if !offset.is_finite() {
        return format_error(format!("HDF5 attribute {prefix}/offset must be finite"));
    }
    Ok(ChannelInfo {
        group_name: group_name.into(),
        original_name: required_nonempty_string(channel, "original_name")?,
        description: required_string(channel, "description")?,
        unit: required_string(channel, "unit")?,
        offset,
        column: required_u64(channel, "column")?,
    })
}

fn validate_test_info(group: &Group) -> Result<Vec<TestInfo>, MeasurementHdf5Error> {
    let mut info = Vec::new();
    for group_name in group.member_names()? {
        validate_safe_group_name(&group_name, "/measurements/test_info")?;
        let entry = required_group(group, &group_name, "/measurements/test_info")?;
        info.push(TestInfo {
            group_name,
            name: required_string(&entry, "name")?,
            description: required_string(&entry, "description")?,
            data_type: required_string(&entry, "data_type")?,
            unit: required_string(&entry, "unit")?,
            value: required_string(&entry, "value")?,
        });
    }
    Ok(info)
}

fn validate_values(
    dataset: &hdf5::Dataset,
    sample_count: usize,
    path: &str,
) -> Result<(), MeasurementHdf5Error> {
    if !dataset.dtype()?.is::<f64>() {
        return format_error(format!("{path} must have dtype float64"));
    }
    if dataset.ndim() != 1 || dataset.shape()[0] != sample_count {
        return format_error(format!(
            "{path} must be one-dimensional with {sample_count} entries"
        ));
    }
    Ok(())
}

fn validate_safe_group_name(name: &str, parent: &str) -> Result<(), MeasurementHdf5Error> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return format_error(format!("{parent}/{name} is not a safe group name"));
    }
    Ok(())
}

fn required_group(parent: &Group, name: &str, path: &str) -> Result<Group, MeasurementHdf5Error> {
    parent.group(name).map_err(|_| {
        MeasurementHdf5Error::Format(Hdf5FormatError(format!("missing HDF5 group: {path}")))
    })
}

fn dataset(group: &Group, name: &str, prefix: &str) -> Result<hdf5::Dataset, MeasurementHdf5Error> {
    group.dataset(name).map_err(|_| {
        MeasurementHdf5Error::Format(Hdf5FormatError(format!(
            "{prefix}/{name} is missing or not a dataset"
        )))
    })
}

fn platform_count(value: u64, name: &str) -> Result<usize, MeasurementHdf5Error> {
    usize::try_from(value).map_err(|_| {
        MeasurementHdf5Error::Format(Hdf5FormatError(format!("{name} exceeds platform size")))
    })
}

fn required_u32(location: &Location, name: &str) -> Result<u32, MeasurementHdf5Error> {
    location
        .attr(name)
        .map_err(|_| missing_attribute(name))?
        .read_scalar::<u32>()
        .map_err(|_| invalid_attribute(name, "uint32"))
}

fn required_u64(location: &Location, name: &str) -> Result<u64, MeasurementHdf5Error> {
    location
        .attr(name)
        .map_err(|_| missing_attribute(name))?
        .read_scalar::<u64>()
        .map_err(|_| invalid_attribute(name, "uint64"))
}

fn required_f64(location: &Location, name: &str) -> Result<f64, MeasurementHdf5Error> {
    location
        .attr(name)
        .map_err(|_| missing_attribute(name))?
        .read_scalar::<f64>()
        .map_err(|_| invalid_attribute(name, "float64"))
}

fn required_string(location: &Location, name: &str) -> Result<String, MeasurementHdf5Error> {
    optional_string(location, name)?.ok_or_else(|| missing_attribute(name))
}

fn required_nonempty_string(
    location: &Location,
    name: &str,
) -> Result<String, MeasurementHdf5Error> {
    let value = required_string(location, name)?;
    if value.is_empty() {
        return format_error(format!("HDF5 attribute {name} must not be empty"));
    }
    Ok(value)
}

fn optional_string(
    location: &Location,
    name: &str,
) -> Result<Option<String>, MeasurementHdf5Error> {
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
        .map_err(|_| invalid_attribute(name, "a variable-length UTF-8 string"))
}

fn missing_attribute(name: &str) -> MeasurementHdf5Error {
    MeasurementHdf5Error::Format(Hdf5FormatError(format!("missing HDF5 attribute: {name}")))
}

fn invalid_attribute(name: &str, expected: &str) -> MeasurementHdf5Error {
    MeasurementHdf5Error::Format(Hdf5FormatError(format!(
        "HDF5 attribute {name} must be {expected}"
    )))
}

fn format_error<T>(message: String) -> Result<T, MeasurementHdf5Error> {
    Err(MeasurementHdf5Error::Format(Hdf5FormatError(message)))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hdf5::types::VarLenUnicode;
    use hdf5::{File, Group, Location};
    use tempfile::TempDir;

    use super::*;

    fn fixture() -> (TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("measurement.h5");
        let file = File::create(&path).unwrap();
        scalar(&file, "format_version", 1_u32);
        scalar(&file, "sample_count", 3_u64);
        scalar(&file, "channel_count", 2_u64);
        string(&file, "source_kind", "measurement");
        string(&file, "source_file", "run.csv");
        string(&file, "encoding", "utf-8");
        let measurements = file.create_group("measurements").unwrap();
        measurements
            .new_dataset_builder()
            .with_data(&[0.0, f64::NAN, f64::INFINITY])
            .create("sample_axis")
            .unwrap();
        let channels = measurements.create_group("channels").unwrap();
        channel(
            &channels,
            "front_force",
            "Front force",
            "Force",
            "N",
            0.0,
            4,
            &[1.0, f64::NAN, 3.0],
        );
        channel(
            &channels,
            "rear_force",
            "Rear force",
            "Force",
            "N",
            0.25,
            9,
            &[f64::NEG_INFINITY, 5.0, 6.0],
        );
        let test_info = measurements.create_group("test_info").unwrap();
        let temperature = test_info.create_group("temperature").unwrap();
        string(&temperature, "name", "Temperature");
        string(&temperature, "description", "Ambient temperature");
        string(&temperature, "data_type", "float");
        string(&temperature, "unit", "C");
        string(&temperature, "value", "22.5");
        file.flush().unwrap();
        (directory, path)
    }

    #[allow(clippy::too_many_arguments)]
    fn channel(
        channels: &Group,
        group_name: &str,
        original_name: &str,
        description: &str,
        unit: &str,
        offset: f64,
        column: u64,
        values: &[f64],
    ) {
        let channel = channels.create_group(group_name).unwrap();
        channel
            .new_dataset_builder()
            .with_data(values)
            .create("values")
            .unwrap();
        string(&channel, "original_name", original_name);
        string(&channel, "description", description);
        string(&channel, "unit", unit);
        scalar(&channel, "offset", offset);
        scalar(&channel, "column", column);
    }

    fn scalar<T: hdf5::H5Type>(location: &Location, name: &str, value: T) {
        location
            .new_attr::<T>()
            .create(name)
            .unwrap()
            .write_scalar(&value)
            .unwrap();
    }

    fn string(location: &Location, name: &str, value: &str) {
        location
            .new_attr::<VarLenUnicode>()
            .shape(())
            .create(name)
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str(value).unwrap())
            .unwrap();
    }

    #[test]
    fn reads_valid_file_and_preserves_non_finite_samples() {
        let (_directory, path) = fixture();
        let file = MeasurementFile::open(&path).unwrap();
        assert_eq!(file.summary().channel_count, 2);
        assert_eq!(file.summary().encoding.as_deref(), Some("utf-8"));
        assert_eq!(file.matching_channels("force").len(), 2);
        assert_eq!(
            file.resolve_channel("/measurements/channels/front_force")
                .unwrap()
                .column,
            4
        );
        assert!(file.read_sample_axis().unwrap()[1].is_nan());
        let channel = file.read_channel("Front force", Some("N")).unwrap();
        assert!(channel.values[1].is_nan());
        assert_eq!(file.test_info().len(), 1);
        assert_eq!(file.named_test_info("Temperature").unwrap().value, "22.5");
    }

    #[test]
    fn reports_lookup_and_unit_diagnostics() {
        let (_directory, path) = fixture();
        let file = MeasurementFile::open(&path).unwrap();
        assert!(matches!(
            file.resolve_channel("missing"),
            Err(MeasurementHdf5Error::NotFound(_))
        ));
        assert!(matches!(
            file.read_channel("front_force", Some("mm")),
            Err(MeasurementHdf5Error::Unit(_))
        ));
        assert!(matches!(
            file.named_test_info("missing"),
            Err(MeasurementHdf5Error::NotFound(_))
        ));
    }

    #[test]
    fn rejects_malformed_contracts() {
        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        file.attr("source_kind")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("simulation").unwrap())
            .unwrap();
        drop(file);
        assert!(
            MeasurementFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("source_kind")
        );

        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        file.group("measurements/channels/front_force")
            .unwrap()
            .attr("offset")
            .unwrap()
            .write_scalar(&f64::NAN)
            .unwrap();
        drop(file);
        assert!(
            MeasurementFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("offset must be finite")
        );

        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        file.attr("channel_count")
            .unwrap()
            .write_scalar(&1_u64)
            .unwrap();
        drop(file);
        assert!(
            MeasurementFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("channel_count is 1")
        );

        let (_directory, path) = fixture();
        let file = File::open_rw(&path).unwrap();
        let channels = file.group("measurements/channels").unwrap();
        channels.unlink("front_force").unwrap();
        channels.create_group("bad name").unwrap();
        drop(file);
        assert!(
            MeasurementFile::open(&path)
                .unwrap_err()
                .to_string()
                .contains("not a safe group name")
        );
    }
}
