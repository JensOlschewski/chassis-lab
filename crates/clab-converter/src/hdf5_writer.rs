use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use hdf5::types::VarLenUnicode;
use hdf5::{File, Location};

use crate::model::AdamsResult;

pub fn write_file(path: &Path, source: &Path, result: &AdamsResult) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("failed to create HDF5 file '{}'", path.display()))?;

    file.new_attr::<u64>()
        .create("sample_count")?
        .write_scalar(&(result.sample_count as u64))?;
    file.new_attr::<u64>()
        .create("signal_count")?
        .write_scalar(&(result.signal_count() as u64))?;
    file.new_attr::<u32>()
        .create("schema_version")?
        .write_scalar(&1u32)?;
    write_string_attribute(&file, "source_kind", "simulation")?;
    write_string_attribute(&file, "source_file", &source.display().to_string())?;
    let axis = file.create_group("axis")?;
    write_string_attribute(&axis, "kind", "sample_index")?;
    write_string_attribute(&axis, "unit", "")?;
    axis.new_dataset_builder()
        .with_data(&(0..result.sample_count as u64).collect::<Vec<_>>())
        .create("values")?;
    let signals_group = file.create_group("signals")?;
    let simulation = file.create_group("simulation")?;
    let mut used_signal_names = HashMap::new();

    for entity in &result.entities {
        for component in &entity.components {
            let preferred_name = format!(
                "{}_{}",
                entity.source_path.as_deref().unwrap_or(&entity.name),
                component.name
            );
            let group_name = unique_name(safe_name(&preferred_name, true), &mut used_signal_names);
            let signal = signals_group.create_group(&group_name)?;
            signal
                .new_dataset_builder()
                .with_data(&component.values)
                .create("values")?;
            write_string_attribute(&signal, "original_name", &component.name)?;
            write_string_attribute(&signal, "unit", component.units.as_deref().unwrap_or(""))?;
            write_string_attribute(
                &signal,
                "source_path",
                entity.source_path.as_deref().unwrap_or(&entity.name),
            )?;
            write_string_attribute(&signal, "entity_name", &entity.name)?;
            write_string_attribute(&signal, "component", &component.name)?;
            signal
                .new_attr::<u64>()
                .create("adams_id")?
                .write_scalar(&(component.adams_id as u64))?;
            if let Some(object_id) = entity.object_id {
                signal
                    .new_attr::<u64>()
                    .create("object_id")?
                    .write_scalar(&object_id)?;
            }
        }
    }
    write_string_attribute(&simulation, "format", "adams-res")?;

    file.flush()?;
    Ok(())
}

/// Write one scalar UTF-8 HDF5 attribute.
pub(crate) fn write_string_attribute(location: &Location, name: &str, value: &str) -> Result<()> {
    let value = as_hdf5_string(value)?;
    location
        .new_attr::<VarLenUnicode>()
        .shape(())
        .create(name)?
        .write_scalar(&value)?;
    Ok(())
}

fn as_hdf5_string(value: &str) -> Result<VarLenUnicode> {
    VarLenUnicode::from_str(value)
        .with_context(|| format!("'{value}' cannot be represented as an HDF5 UTF-8 string"))
}

/// Build an HDF5-safe name. Simulation sources trim surrounding whitespace;
/// measurement channel names retain it before unsupported bytes become `_`.
pub(crate) fn safe_name(name: &str, trim_surrounding_whitespace: bool) -> String {
    let name = if trim_surrounding_whitespace {
        name.trim()
    } else {
        name
    };
    let name: String = name
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                byte as char
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "unnamed".to_owned()
    } else {
        name
    }
}

/// Return `base`, or its stable `_N` collision suffix, and record its use.
pub(crate) fn unique_name(base: String, used: &mut HashMap<String, usize>) -> String {
    let count = used.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}_{count}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{safe_name, unique_name};

    #[test]
    fn creates_safe_unique_group_names() {
        let mut used = HashMap::new();
        assert_eq!(safe_name("part/wheel", true), "part_wheel");
        assert_eq!(safe_name("", true), "unnamed");
        assert_eq!(safe_name(" wheel ", true), "wheel");
        assert_eq!(safe_name(" wheel ", false), "_wheel_");
        assert_eq!(unique_name("wheel".into(), &mut used), "wheel");
        assert_eq!(unique_name("wheel".into(), &mut used), "wheel_2");
    }
}
