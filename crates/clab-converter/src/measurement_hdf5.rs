use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use hdf5::{File, Location};

use clab_core::canonical_hdf5::SCHEMA_VERSION;

use crate::acd::{self, AcdMeasurement};
use crate::hdf5_writer::{safe_name, unique_name, write_string_attribute};
use crate::identity::{self, Identity};

/// Imports an ACD measurement into canonical HDF5. The exact `time` channel is sample axis.
///
/// Rejects a declared `identity` the data contradicts before writing anything.
pub fn import_acd(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    overwrite: bool,
    identity: &Identity,
) -> Result<()> {
    let source = source.as_ref();
    let measurement = acd::parse_file(source)
        .with_context(|| format!("failed to parse ACD file '{}'", source.display()))?;
    identity::check_acd_identity(&measurement, identity, &source.display().to_string())?;
    crate::write_atomically(output.as_ref(), overwrite, |temporary| {
        write_file(temporary, source, &measurement, identity)
    })
}

pub fn write_file(
    path: &Path,
    source: &Path,
    measurement: &AcdMeasurement,
    identity: &Identity,
) -> Result<()> {
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
    scalar(&file, "schema_version", SCHEMA_VERSION)?;
    scalar(&file, "sample_count", sample_count as u64)?;
    scalar(&file, "signal_count", measurement.channels.len() as u64)?;
    write_string_attribute(&file, "source_kind", "measurement")?;
    write_string_attribute(&file, "domain", &identity.domain)?;
    if let Some(test) = &identity.test {
        write_string_attribute(&file, "test", test)?;
    }
    write_string_attribute(&file, "source_file", &source.display().to_string())?;
    write_string_attribute(&file, "encoding", &measurement.encoding)?;
    write_string_attribute(&file, "dialect", &measurement.dialect)?;

    let axis = file.create_group("axis")?;
    write_string_attribute(&axis, "kind", "time")?;
    write_string_attribute(&axis, "unit", &time.unit)?;
    axis.new_dataset_builder()
        .with_data(&time.values)
        .create("values")?;
    let signals = file.create_group("signals")?;
    let mut used_names = HashMap::new();
    for channel in &measurement.channels {
        let group = signals.create_group(&unique_name(
            safe_name(&channel.name, false),
            &mut used_names,
        ))?;
        group
            .new_dataset_builder()
            .with_data(&channel.values)
            .create("values")?;
        write_string_attribute(&group, "original_name", &channel.name)?;
        write_string_attribute(&group, "description", &channel.description)?;
        write_string_attribute(&group, "unit", &channel.unit)?;
        scalar(&group, "offset", channel.offset)?;
        scalar(&group, "column", channel.column)?;
    }
    let metadata = file.create_group("measurement")?;
    if !measurement.test_info.is_empty() {
        let test_info = metadata.create_group("test_info")?;
        let mut used_names = HashMap::new();
        for info in &measurement.test_info {
            let group = test_info
                .create_group(&unique_name(safe_name(&info.name, false), &mut used_names))?;
            write_string_attribute(&group, "name", &info.name)?;
            write_string_attribute(&group, "description", &info.description)?;
            write_string_attribute(&group, "data_type", &info.data_type)?;
            write_string_attribute(&group, "unit", &info.unit)?;
            write_string_attribute(&group, "value", &info.value)?;
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
