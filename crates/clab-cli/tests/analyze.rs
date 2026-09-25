use std::{fs, path::Path, process::Command};

use hdf5::File;
use serde_json::Value;
use tempfile::tempdir;

fn clab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_clab"))
}

fn clear_simulation_units(path: &Path) {
    let staging = path.with_extension("units.h5");
    fs::copy(path, &staging).unwrap();
    let file = File::open_rw(&staging).unwrap();
    let signals = file.group("signals").unwrap();
    for name in signals.member_names().unwrap() {
        let group = signals.group(&name).unwrap();
        group
            .attr("unit")
            .unwrap()
            .write_scalar(&"".parse::<hdf5::types::VarLenUnicode>().unwrap())
            .unwrap();
        drop(group);
    }
    drop(signals);
    file.close().unwrap();
    fs::copy(&staging, path).unwrap();
    fs::remove_file(staging).unwrap();
}

#[test]
fn analyzes_svc_with_report_bundle() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-simulation-svc.res");
    assert!(config.is_file(), "missing {}", config.display());
    assert!(source.is_file(), "missing {}", source.display());
    let directory = tempdir().unwrap();
    let input = directory.path().join("simulation.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    let analysis_output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
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
fn analyzes_svc_measurement_reference_with_stdout_metrics() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("example-config.toml");
    let input = examples.join("input/example-measurement-svc.json");
    if !config.is_file() || !input.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stdout.contains("SVC reference analysis: 29 metrics"));
    assert!(stdout.contains("  front_left_mass_kg = 420.0 kg"));
    assert!(stdout.contains("  cg_height_mm = missing mm"));
    assert!(stderr.contains("16 analysis warnings; details in"));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let metrics = report["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 29);
    assert_eq!(metrics[0]["metric"], "front_left_mass_kg");
    assert!(
        metrics
            .iter()
            .any(|metric| { metric["metric"] == "cg_height_mm" && metric["value"].is_null() })
    );
    assert_eq!(report["warnings"].as_array().unwrap().len(), 16);

    let verbose = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(verbose.status.success());
    assert!(
        String::from_utf8_lossy(&verbose.stderr)
            .contains("SVC metric 'cg_height_mm' is missing from measurement reference")
    );
}

#[test]
fn rejects_svc_json_without_source_kind() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    assert!(config.is_file(), "missing {}", config.display());
    let directory = tempdir().unwrap();
    let input = directory.path().join("missing-source-kind.json");
    fs::write(&input, r#"{"schema_version": 1}"#).unwrap();

    let result = clab()
        .args(["analyze", config.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("missing-source-kind.json"));
    assert!(stderr.contains("source_kind is required"));
    assert!(stderr.contains("allowed values: simulation, measurement"));
}

#[test]
fn rejects_missing_normalized_input_without_hdf5_diagnostics() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    assert!(config.is_file(), "missing {}", config.display());
    let directory = tempdir().unwrap();
    let input = directory.path().join("missing.h5");

    let result = clab()
        .args(["analyze", config.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("input file does not exist or is not a file"));
    assert!(stderr.contains(input.to_str().unwrap()));
    assert!(!stderr.contains("H5Fopen"));
}

#[test]
fn rejects_svc_measurement_hdf5_without_json_reference_parsing() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-measurement-knc-vertical.acd");
    assert!(config.is_file(), "missing {}", config.display());
    assert!(source.is_file(), "missing {}", source.display());

    let directory = tempdir().unwrap();
    let input = directory.path().join("measurement.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );

    let result = clab()
        .args(["analyze", config.to_str().unwrap(), input.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("SVC analysis does not support measurement HDF5 input"));
    assert!(!stderr.contains("invalid SVC reference JSON"));
}

#[test]
fn analyzes_svc_hdf5_with_report_bundle() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-simulation-svc.res");
    if !config.is_file() || !source.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let input = directory.path().join("svc.simulation.h5");
    let result = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("SVC analysis"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("KnC"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("warning:"));
    for name in ["report.json", "metrics.csv", "svc.json"] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    assert!(!output.join("knc-vertical.json").exists());
}

#[test]
fn analyzes_svc_hdf5_run_with_hdf5_analyzer() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-simulation-svc.res");
    if !config.is_file() || !source.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let input = directory.path().join("svc.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(converted.status.success());
    let run = directory.path().join("run.toml");
    fs::write(&run, format!("[input.svc]\nfile = {input:?}\n")).unwrap();
    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            "-r",
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
    assert!(!output.join("knc-vertical.json").exists());
}

#[test]
fn analyzes_svc_hdf5_with_missing_units_and_warns() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-simulation-svc.res");
    if !config.is_file() || !source.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let input = directory.path().join("svc.simulation.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(converted.status.success());
    clear_simulation_units(&input);

    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("SVC analysis"));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("analysis warnings; details in"));
    assert!(!stderr.contains("no HDF5 unit metadata"));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert!(!warnings.is_empty());
    assert!(
        warnings
            .iter()
            .all(|warning| warning.as_str().unwrap().contains("mapped unit"))
    );
    assert_eq!(
        warnings.len(),
        warnings
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );

    let verbose = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(verbose.status.success());
    assert!(String::from_utf8_lossy(&verbose.stderr).contains("no HDF5 unit metadata"));
}

#[test]
fn analyze_force_rejects_output_containing_input() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-simulation-svc.res");
    assert!(config.is_file(), "missing {}", config.display());
    assert!(source.is_file(), "missing {}", source.display());
    let directory = tempdir().unwrap();
    let simulation = directory.path().join("simulation.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-o",
            simulation.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(converted.status.success());
    let output = directory.path().join("analysis");
    fs::create_dir(&output).unwrap();
    let input = output.join("input.h5");
    fs::copy(&simulation, &input).unwrap();
    let rejected = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("must not contain an input"));
    assert_eq!(fs::read(input).unwrap(), fs::read(simulation).unwrap());
}

#[test]
fn analyzes_run_with_svc_and_vertical_in_one_bundle() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let svc = examples.join("input/example-simulation-svc.res");
    let vertical = examples.join("input/example-simulation-knc-vertical.res");
    if !config.is_file() || !svc.is_file() || !vertical.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let vertical_simulation = directory.path().join("vertical.h5");
    let result = clab()
        .args([
            "convert",
            vertical.to_str().unwrap(),
            "-o",
            vertical_simulation.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let svc_simulation = directory.path().join("svc.h5");
    let result = clab()
        .args([
            "convert",
            svc.to_str().unwrap(),
            "-o",
            svc_simulation.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let run = directory.path().join("run.toml");
    fs::write(
        &run,
        format!(
            "[input.svc]\nfile = {:?}\n\n[input.knc.vertical]\nfile = {:?}\n",
            svc_simulation, vertical_simulation
        ),
    )
    .unwrap();
    let output = directory.path().join("analysis");
    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            "-r",
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
