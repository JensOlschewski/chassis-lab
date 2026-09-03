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
    file.new_attr::<u64>()
        .create("values_per_sample")?
        .write_scalar(&(result.values_per_sample as u64))?;
    file.new_attr::<u32>()
        .create("format_version")?
        .write_scalar(&3u32)?;
    write_string_attribute(&file, "source_file", &source.display().to_string())?;

    let results_group = file.create_group("results")?;
    let mut used_entity_names = HashMap::new();

    for entity in &result.entities {
        let preferred_name = entity.source_path.as_deref().unwrap_or(&entity.name);
        let group_name = unique_name(sanitize_group_name(preferred_name), &mut used_entity_names);
        let entity_group = results_group.create_group(&group_name)?;

        write_string_attribute(&entity_group, "original_name", &entity.name)?;
        if let Some(source_path) = &entity.source_path {
            write_string_attribute(&entity_group, "source_path", source_path)?;
        }
        if let Some(object_id) = entity.object_id {
            entity_group
                .new_attr::<u64>()
                .create("object_id")?
                .write_scalar(&object_id)?;
        }

        let component_count = entity.components.len();
        let mut values = Vec::with_capacity(result.sample_count * component_count);
        for sample_index in 0..result.sample_count {
            for component in &entity.components {
                values.push(component.values[sample_index]);
            }
        }

        entity_group
            .new_dataset::<f64>()
            .shape((result.sample_count, component_count))
            .create("values")?
            .write_raw(&values)?;

        let component_names = entity
            .components
            .iter()
            .map(|component| as_hdf5_string(&component.name))
            .collect::<Result<Vec<_>>>()?;
        let units = entity
            .components
            .iter()
            .map(|component| as_hdf5_string(component.units.as_deref().unwrap_or("")))
            .collect::<Result<Vec<_>>>()?;
        let adams_ids = entity
            .components
            .iter()
            .map(|component| component.adams_id as u64)
            .collect::<Vec<_>>();

        entity_group
            .new_dataset_builder()
            .with_data(&component_names)
            .create("component_names")?;
        entity_group
            .new_dataset_builder()
            .with_data(&units)
            .create("units")?;
        entity_group
            .new_dataset_builder()
            .with_data(&adams_ids)
            .create("adams_ids")?;
    }

    file.flush()?;
    Ok(())
}

fn write_string_attribute(location: &Location, name: &str, value: &str) -> Result<()> {
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

fn sanitize_group_name(name: &str) -> String {
    let name = name.trim().replace(['/', '\0'], "_");
    if name.is_empty() {
        "unnamed".to_owned()
    } else {
        name
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{sanitize_group_name, unique_name};

    #[test]
    fn creates_safe_unique_group_names() {
        let mut used = HashMap::new();
        assert_eq!(sanitize_group_name("part/wheel"), "part_wheel");
        assert_eq!(sanitize_group_name(""), "unnamed");
        assert_eq!(unique_name("wheel".into(), &mut used), "wheel");
        assert_eq!(unique_name("wheel".into(), &mut used), "wheel_2");
    }
}
