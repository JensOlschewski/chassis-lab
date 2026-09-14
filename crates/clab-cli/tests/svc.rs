use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn fixture_case(directory: &std::path::Path, failing: bool) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let reference = fs::read_to_string(root.join("example-measurement-svc.json")).unwrap();
    let simulation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/example-simulation-svc.json");
    let signals = root.join("example-signals.toml");
    let tolerances = root.join("example-svc-tolerances.toml");
    let reference = if failing {
        reference.replace("\"front_track_mm\": 1500.0", "\"front_track_mm\": 1490.0")
    } else {
        reference
    };
    let reference_path = directory.join("reference.json");
    fs::write(&reference_path, reference).unwrap();
    let config = format!(
        "[meta]\nname = 'example-config'\nloadcase = 'example-loadcase'\n[physics]\ngravity = 9.80665\n[input.svc.simulation]\ninput_file = '{}'\n[input.svc.measurement]\ninput_file = '{}'\n[mappings.svc]\nfile = '{}'\n[validation.svc]\ntolerances = '{}'\n",
        simulation.display(),
        reference_path.display(),
        signals.display(),
        tolerances.display()
    );
    let path = directory.join("case.toml");
    fs::write(&path, config).unwrap();
    path
}

fn command(config: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.arg("validate").arg(config);
    command.env("PYTHONHOME", "/path/that/does/not/exist");
    command
}

#[test]
fn validates_svc_and_writes_deterministic_reports_without_python() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let output = directory.path().join("reports");
    let json = output.join("svc.json");
    let csv = output.join("svc.csv");
    let result = command(&config)
        .args(["--output-dir", output.to_str().unwrap()])
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
            .starts_with("case,loadcase,metric,unit")
    );

    let rejected = command(&config)
        .arg("--output-dir")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    assert_eq!(
        command(&config)
            .arg("--output-dir")
            .arg(&output)
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
    let csv = directory.path().join("report.csv");
    let result = command(&config)
        .arg("--json-output")
        .arg(signal_map)
        .arg("--csv-output")
        .arg(csv)
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("must not replace an input"));
}

#[test]
fn creates_nested_report_output_parents() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let output = directory.path().join("nested/reports");
    let result = command(&config)
        .arg("--output-dir")
        .arg(&output)
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(0));
    assert!(output.join("svc.json").is_file());

    let json = directory.path().join("individual/reports/report.json");
    let csv = directory.path().join("individual/reports/report.csv");
    let result = command(&config)
        .arg("--json-output")
        .arg(&json)
        .arg("--csv-output")
        .arg(&csv)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert!(json.is_file() && csv.is_file());

    let result = command(&config)
        .arg("--json-output")
        .arg(directory.path().join("ignored.json"))
        .arg("--csv-output")
        .arg(directory.path().join("ignored.csv"))
        .arg("--plot-output")
        .arg(directory.path().join("ignored.svg"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires selecting KnC"));
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

#[test]
fn selects_svc_and_rejects_unconfigured_knc() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    assert!(
        command(&config)
            .args(["-d", "svc"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let result = command(&config).args(["-d", "knc"]).output().unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("not configured"));
    let result = command(&config)
        .args(["-d", "svc", "-t", "vertical"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires selecting"));
}

#[test]
fn check_runs_svc_preflight_without_python() {
    let directory = tempdir().unwrap();
    let config = fixture_case(directory.path(), false);
    let mut check = Command::new(env!("CARGO_BIN_EXE_clab"));
    check
        .args(["check", config.to_str().unwrap()])
        .env("PYTHONHOME", "/path/that/does/not/exist");
    assert!(check.output().unwrap().status.success());

    let reference = directory.path().join("reference.json");
    fs::write(
        &reference,
        fs::read_to_string(&reference)
            .unwrap()
            .replace("\"example-config\"", "\"wrong\""),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args(["check", config.to_str().unwrap()])
        .env("PYTHONHOME", "/path/that/does/not/exist")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("identity"));
}
