use std::{fs, path::Path, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn clab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_clab"))
}

#[test]
fn analyzes_svc_with_report_bundle() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("example-config.toml");
    let input = examples.join("input/example-simulation-svc.json");
    if !config.is_file() || !input.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let analysis_output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-o",
            analysis_output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for name in ["report.json", "metrics.csv", "svc.json"] {
        assert!(analysis_output.join(name).is_file(), "missing {name}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(analysis_output.join("report.json")).unwrap())
            .unwrap();
    assert_eq!(report["command"], "analyze");
    assert!(report["metrics"].as_array().unwrap().len() > 20);
}

#[test]
fn analyze_force_rejects_output_containing_input() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("example-config.toml");
    let source = examples.join("input/example-simulation-svc.json");
    if !config.is_file() || !source.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let output = directory.path().join("analysis");
    fs::create_dir(&output).unwrap();
    let input = output.join("input.json");
    fs::copy(&source, &input).unwrap();
    let rejected = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("must not contain an input"));
    assert_eq!(
        fs::read_to_string(input).unwrap(),
        fs::read_to_string(source).unwrap()
    );
}

#[test]
fn analyzes_run_with_svc_and_vertical_in_one_bundle() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let run = examples.join("run.toml");
    if !config.is_file() || !run.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for name in [
        "report.json",
        "metrics.csv",
        "svc.json",
        "knc-vertical.json",
    ] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"].as_array().unwrap().len(), 2);
}

#[test]
fn compares_svc_inputs_and_protects_reports_and_inputs() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let simulation = examples.join("input/example-simulation-svc.json");
    if !config.is_file() || !simulation.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let candidate = directory.path().join("candidate.json");
    fs::write(
        &candidate,
        fs::read_to_string(&simulation)
            .unwrap()
            .replace("4118.793", "4200.0"),
    )
    .unwrap();
    let output = directory.path().join("reports");
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
            simulation.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for name in ["report.json", "metrics.csv", "svc.json"] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["command"], "compare");
    assert_eq!(report["status"], "informational");
    assert!(report.get("passed").is_none());
    assert_eq!(report["candidates"].as_array().unwrap().len(), 2);
    assert!(
        report["candidates"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| metric["status"] == "informational")
    );
    let mut csv = csv::Reader::from_path(output.join("metrics.csv")).unwrap();
    assert_eq!(
        csv.headers().unwrap().iter().collect::<Vec<_>>(),
        vec![
            "baseline",
            "candidate",
            "domain",
            "test",
            "metric",
            "unit",
            "baseline_value",
            "candidate_value",
            "difference",
            "absolute_tolerance",
            "relative_percent",
            "allowed_difference",
            "status",
        ]
    );
    assert!(
        csv.records()
            .all(|record| record.unwrap().get(12) == Some("informational"))
    );

    let rejected = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    assert!(
        clab()
            .args([
                "compare",
                config.to_str().unwrap(),
                simulation.to_str().unwrap(),
                candidate.to_str().unwrap(),
                "-o",
                output.to_str().unwrap(),
                "--force"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );

    let file_output = directory.path().join("report-file");
    fs::write(&file_output, "keep").unwrap();
    let rejected = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "-o",
            file_output.to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("regular file"));
    assert_eq!(fs::read_to_string(file_output).unwrap(), "keep");

    let protected_input = directory.path().join("report.json");
    fs::copy(&simulation, &protected_input).unwrap();
    let rejected = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            protected_input.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "-o",
            directory.path().to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("must not replace an input"));
}

#[test]
fn compares_run_toml_and_returns_one_for_failed_tolerance() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let run = examples.join("run.toml");
    if !config.is_file() || !run.is_file() {
        return;
    }
    assert!(
        clab()
            .args([
                "compare",
                config.to_str().unwrap(),
                run.to_str().unwrap(),
                run.to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .success()
    );

    let directory = tempdir().unwrap();
    let tolerance = directory.path().join("tolerances.toml");
    fs::write(
        &tolerance,
        "[svc.validation.tolerances.absolute]\ntotal_mass_kg = 0.0\n",
    )
    .unwrap();
    let candidate = directory.path().join("candidate.json");
    let simulation = examples.join("input/example-simulation-svc.json");
    fs::write(
        &candidate,
        fs::read_to_string(simulation)
            .unwrap()
            .replace("4118.793", "4200.0"),
    )
    .unwrap();
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "--tolerances",
            tolerance.to_str().unwrap(),
            "-o",
            directory.path().join("failed-report").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("failed"));
    let report: Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("failed-report/report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(report["status"], "failed");
    let metric = report["candidates"][0]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["metric"] == "total_mass_kg")
        .unwrap();
    assert_eq!(metric["absolute_tolerance"], 0.0);
    assert!(metric["relative_percent"].is_null());
    assert_eq!(metric["allowed_difference"], 0.0);
    assert_eq!(metric["status"], "failed");

    let output = directory.path().join("passed-report");
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            run.to_str().unwrap(),
            "--tolerances",
            tolerance.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "passed");
}
