use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::model::AdamsResult;

pub fn write_file(path: &Path, source: &Path, result: &AdamsResult) -> Result<()> {
    let document = json!({
        "schema_version": 1,
        "source_file": source.display().to_string(),
        "sample_count": result.sample_count,
        "values_per_sample": result.values_per_sample,
        "entities": result.entities.iter().map(|entity| json!({
            "name": entity.name,
            "source_path": entity.source_path,
            "object_id": entity.object_id,
            "components": entity.components.iter().map(|component| json!({
                "name": component.name,
                "adams_id": component.adams_id,
                "units": component.units,
                "values": component.values,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&document)? + "\n";
    std::fs::write(path, text)
        .with_context(|| format!("failed to create JSON file '{}'", path.display()))
}
