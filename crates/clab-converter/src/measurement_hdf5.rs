use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use hdf5::types::VarLenUnicode;
use hdf5::{File, Location};

use crate::acd::{self, AcdMeasurement};

/// Imports an ACD measurement into canonical HDF5. The exact `time` channel is sample axis.
pub fn import_acd(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
) -> Result<()> {
    let source = source.as_ref();
    let measurement = acd::parse_file(source)
        .with_context(|| format!("failed to parse ACD file '{}'", source.display()))?;
    crate::write_atomically(output.as_ref(), overwrite, |temporary| {
        write_file(temporary, source, &measurement)
    })
}

pub fn write_file(path: &Path, source: &Path, measurement: &AcdMeasurement) -> Result<()> {
    let time = measurement
        .channels
        .iter()
        .find(|channel| channel.name == "time")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "ACD measurement has no channel named exactly 'time'; sample axis requires it"
            )
        })?;
    let sample_count = time.values.len();
    if measurement
        .channels
        .iter()
        .any(|channel| channel.values.len() != sample_count)
    {
        bail!("ACD measurement channels have inconsistent sample counts");
    }

    let file = File::create(path)
        .with_context(|| format!("failed to create HDF5 file '{}'", path.display()))?;
    scalar(&file, "schema_version", 1_u32)?;
    scalar(&file, "sample_count", sample_count as u64)?;
    scalar(&file, "signal_count", measurement.channels.len() as u64)?;
    string(&file, "source_kind", "measurement")?;
    string(&file, "source_file", &source.display().to_string())?;
    string(&file, "encoding", &measurement.encoding)?;
    string(&file, "dialect", &measurement.dialect)?;

    let axis = file.create_group("axis")?;
    string(&axis, "kind", "time")?;
    string(&axis, "unit", &time.unit)?;
    axis.new_dataset_builder()
        .with_data(&time.values)
        .create("values")?;
    let signals = file.create_group("signals")?;
    let mut used_names = HashMap::new();
    for channel in &measurement.channels {
        let group =
            signals.create_group(&unique_name(safe_name(&channel.name), &mut used_names))?;
        group
            .new_dataset_builder()
            .with_data(&channel.values)
            .create("values")?;
        string(&group, "original_name", &channel.name)?;
        string(&group, "description", &channel.description)?;
        string(&group, "unit", &channel.unit)?;
        scalar(&group, "offset", channel.offset)?;
        scalar(&group, "column", channel.column)?;
    }
    let metadata = file.create_group("measurement")?;
    if !measurement.test_info.is_empty() {
        let test_info = metadata.create_group("test_info")?;
        let mut used_names = HashMap::new();
        for info in &measurement.test_info {
            let group =
                test_info.create_group(&unique_name(safe_name(&info.name), &mut used_names))?;
            string(&group, "name", &info.name)?;
            string(&group, "description", &info.description)?;
            string(&group, "data_type", &info.data_type)?;
            string(&group, "unit", &info.unit)?;
            string(&group, "value", &info.value)?;
        }
    }
    file.flush()?;
    Ok(())
}

fn scalar<T: hdf5::H5Type>(location: &Location, name: &str, value: T) -> Result<()> {
    location
        .new_attr::<T>()
        .create(name)?
        .write_scalar(&value)?;
    Ok(())
}

fn string(location: &Location, name: &str, value: &str) -> Result<()> {
    let value = VarLenUnicode::from_str(value)
        .with_context(|| format!("'{value}' cannot be represented as an HDF5 UTF-8 string"))?;
    location
        .new_attr::<VarLenUnicode>()
        .shape(())
        .create(name)?
        .write_scalar(&value)?;
    Ok(())
}

fn safe_name(name: &str) -> String {
    let safe: String = name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                byte as char
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "unnamed".into()
    } else {
        safe
    }
}

fn unique_name(base: String, used: &mut HashMap<String, usize>) -> String {
    let count = used.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}_{count}")
    }
}
