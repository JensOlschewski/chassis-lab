use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

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
