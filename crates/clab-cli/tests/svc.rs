use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn fixture_case(directory: &std::path::Path, failing: bool) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = fs::read_to_string(root.join("example-config.toml")).unwrap();
    let reference = fs::read_to_string(root.join("example-measures.json")).unwrap();
    let simulation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../clab-converter/examples/example-svc.h5");
    let signals = root.join("example-signals.toml");
    let reference = if failing {
        reference.replace("\"front_track_mm\": 1500.0", "\"front_track_mm\": 1490.0")
    } else {
        reference
    };
    let reference_path = directory.join("reference.json");
    fs::write(&reference_path, reference).unwrap();
    let config = config
        .replace(
            "\"../crates/clab-converter/examples/example-svc.h5\"",
            &format!("\"{}\"", simulation.display()),
        )
        .replace(
            "\"example-signals.toml\"",
            &format!("\"{}\"", signals.display()),
        )
        .replace(
            "\"example-measures.json\"",
            &format!("\"{}\"", reference_path.display()),
        );
    let path = directory.join("case.toml");
    fs::write(&path, config).unwrap();
    path
}

fn command(config: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.args(["validate", "svc"]).arg(config);
    command.env("PYTHONHOME", "/path/that/does/not/exist");
    command
}

#[test]
fn validates_svc_and_writes_deterministic_reports_without_python() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let json = directory.path().join("report.json");
    let csv = directory.path().join("report.csv");
    let result = command(&config)
        .args([
            "--json-output",
            json.to_str().unwrap(),
            "--csv-output",
            csv.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("validation result: ok"));
    let report: Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["passed"], true);
    let metrics = report["metrics"].as_array().unwrap();
    assert!(
        metrics
            .iter()
            .any(|metric| { metric["name"] == "total_mass_kg" && metric["actual"] == 1600.0 })
    );
    assert!(
        fs::read_to_string(&csv)
            .unwrap()
            .starts_with("case,variant,loadcase,metric,unit")
    );

    let rejected = command(&config)
        .arg("--json-output")
        .arg(&json)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    assert_eq!(
        command(&config)
            .arg("--json-output")
            .arg(&json)
            .arg("--force")
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn rejects_signal_map_as_report_output() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let signal_map = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/example-signals.toml");
    let result = command(&config)
        .arg("--json-output")
        .arg(signal_map)
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("must not replace an input"));
}

#[test]
fn creates_nested_report_output_parents() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let json = directory.path().join("nested/reports/report.json");
    let result = command(&config)
        .arg("--json-output")
        .arg(&json)
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(0));
    assert!(json.is_file());
}

#[test]
fn returns_one_for_tolerance_failure_and_two_for_operational_failure() {
    let directory = tempdir().unwrap();
    let failing = fixture_case(directory.path(), true);
    let result = command(&failing).output().unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("validation result: FAILED"));

    let missing = directory.path().join("missing.toml");
    let result = command(&missing).output().unwrap();
    assert_eq!(result.status.code(), Some(2));
}
