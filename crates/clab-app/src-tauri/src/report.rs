use std::{
    collections::BTreeMap,
    fs::File,
    io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Files of a report bundle the viewer may read: nothing outside the names `clab analyze` writes.
fn is_bundle_file(name: &str) -> bool {
    matches!(
        name,
        "report.json" | "knc-vertical.plots.json" | "knc-roll.plots.json"
    )
}

pub fn read_file(directory: &Path, name: &str) -> Result<String, String> {
    if !is_bundle_file(name) {
        return Err(format!("{name} is not a report file"));
    }
    let path = directory.join(name);
    std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Missing,
    Changed,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ChangedInput {
    path: PathBuf,
    change: Change,
}

/// Every `{path, sha256}` object of a report: the files its results were computed from.
fn collect_inputs(value: &Value, found: &mut BTreeMap<PathBuf, String>) {
    match value {
        Value::Object(map) => match (map.get("path"), map.get("sha256")) {
            (Some(Value::String(path)), Some(Value::String(sha256))) => {
                found.insert(path.into(), sha256.clone());
            }
            _ => map.values().for_each(|child| collect_inputs(child, found)),
        },
        Value::Array(items) => items.iter().for_each(|item| collect_inputs(item, found)),
        _ => {}
    }
}

fn sha256_hex(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// The inputs a report recorded that are gone or differ from their recorded hash.
pub fn changed_inputs(directory: &Path) -> Result<Vec<ChangedInput>, String> {
    let report: Value =
        serde_json::from_str(&read_file(directory, "report.json")?).map_err(|e| e.to_string())?;
    let mut recorded = BTreeMap::new();
    collect_inputs(&report, &mut recorded);
    Ok(recorded
        .into_iter()
        .filter_map(|(path, sha256)| {
            let change = match sha256_hex(&path) {
                Err(_) => Change::Missing,
                Ok(current) if current != sha256 => Change::Changed,
                Ok(_) => return None,
            };
            Some(ChangedInput { path, change })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn flags_missing_and_changed_inputs_only() {
        let directory = tempfile::tempdir().unwrap();
        let [same, changed, missing] =
            ["same", "changed", "missing"].map(|n| directory.path().join(n));
        std::fs::write(&same, "abc").unwrap();
        std::fs::write(&changed, "xyz").unwrap();
        let record = |path: &Path| serde_json::json!({"path": path, "sha256": ABC_SHA256});
        let report = serde_json::json!({
            "inputs": {"config": record(&same), "run": record(&changed)},
            "candidates": [{"input": record(&missing)}, {"input": record(&same)}],
        });
        std::fs::write(directory.path().join("report.json"), report.to_string()).unwrap();

        assert_eq!(
            changed_inputs(directory.path()).unwrap(),
            vec![
                ChangedInput {
                    path: changed,
                    change: Change::Changed
                },
                ChangedInput {
                    path: missing,
                    change: Change::Missing
                },
            ]
        );
    }

    #[test]
    fn reads_only_bundle_files() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("report.json"), "{}").unwrap();
        std::fs::write(directory.path().join("secret.txt"), "x").unwrap();

        assert_eq!(read_file(directory.path(), "report.json").unwrap(), "{}");
        assert!(read_file(directory.path(), "secret.txt").is_err());
        assert!(read_file(directory.path(), "../report.json").is_err());
    }

    #[test]
    fn missing_file_names_its_path() {
        let directory = tempfile::tempdir().unwrap();
        let error = read_file(directory.path(), "knc-roll.plots.json").unwrap_err();
        assert!(error.contains("knc-roll.plots.json"));
    }
}
