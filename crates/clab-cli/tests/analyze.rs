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
            "-d",
            "svc",
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
    let direct_config = directory.path().join("config.toml");
    fs::write(
        &direct_config,
        format!(
            "[physics]\ngravity = 9.80665\n[signals]\nfile = {:?}\n",
            examples.join("mappings.toml")
        ),
    )
    .unwrap();
    let result = clab()
        .args([
            "analyze",
            direct_config.to_str().unwrap(),
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
    assert_eq!(report["case"]["name"], "undefined");
    assert_eq!(report["case"]["loadcase"], "undefined");
    assert!(report["metrics"].as_array().unwrap().len() > 20);
}

#[test]
fn warns_when_run_manifest_uses_schema_version_one() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let directory = tempdir().unwrap();
    let input = directory.path().join("legacy.h5");
    let converted = clab()
        .args([
            "convert",
            examples
                .join("input/example-simulation-svc.res")
                .to_str()
                .unwrap(),
            "-d",
            "svc",
            "-o",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(converted.status.success());
    let file = File::open_rw(&input).unwrap();
    file.attr("schema_version")
        .unwrap()
        .write_scalar(&1_u32)
        .unwrap();
    file.delete_attr("domain").unwrap();
    file.close().unwrap();

    let config = directory.path().join("config.toml");
    fs::write(
        &config,
        format!("[signals]\nfile={:?}\n", examples.join("mappings.toml")),
    )
    .unwrap();
    let run = directory.path().join("run.toml");
    fs::write(&run, format!("[input.svc]\nfile={input:?}\n")).unwrap();
    let result = clab()
        .args(["analyze", config.to_str().unwrap(), run.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("uses schema version 1"));
}

#[test]
fn analyzes_svc_measurement_reference_with_stdout_metrics() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let input = examples.join("input/generated-measurement-svc.json");
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
    assert!(stdout.starts_with("\nSVC\nrunning 29 metrics\n"));
    assert!(stdout.contains("  svc::front_left_mass_kg ... 420.0 kg"));
    assert!(stdout.contains("  svc::cg_height_mm ... missing mm"));
    assert!(stderr.contains("16 analysis warnings; details in"));
    let expected = [
        "front_left_mass_kg",
        "front_right_mass_kg",
        "rear_left_mass_kg",
        "rear_right_mass_kg",
        "front_mass_kg",
        "rear_mass_kg",
        "total_mass_kg",
        "front_distribution_percent",
        "rear_distribution_percent",
        "wheelbase_mm",
        "cg_x_from_front_axle_mm",
        "cg_height_mm",
        "ride_height_front_left_mm",
        "ride_height_front_right_mm",
        "ride_height_rear_left_mm",
        "ride_height_rear_right_mm",
        "front_track_mm",
        "rear_track_mm",
        "toe_front_left_deg",
        "toe_front_right_deg",
        "toe_rear_left_deg",
        "toe_rear_right_deg",
        "camber_front_left_deg",
        "camber_front_right_deg",
        "camber_rear_left_deg",
        "camber_rear_right_deg",
        "inertia_xx_kg_mm2",
        "inertia_yy_kg_mm2",
        "inertia_zz_kg_mm2",
    ];
    let positions = expected
        .iter()
        .map(|metric| stdout.find(&format!("  svc::{metric} ...")).unwrap())
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["case"]["name"], "undefined");
    assert_eq!(report["case"]["loadcase"], "undefined");
    let metrics = report["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 29);
    assert_eq!(
        metrics
            .iter()
            .map(|metric| metric["metric"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected.as_slice()
    );
    let mut csv = csv::Reader::from_path(output.join("metrics.csv")).unwrap();
    assert_eq!(csv.headers().unwrap().get(3), Some("metric"));
    assert_eq!(
        csv.records()
            .map(|record| record.unwrap().get(3).unwrap().to_owned())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|metric| (*metric).to_owned())
            .collect::<Vec<_>>()
    );
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
            .contains("svc: metric 'cg_height_mm' is missing from measurement reference")
    );
}

#[test]
fn analyzes_svc_measurement_run_with_manifest_metadata() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let input = examples.join("input/generated-measurement-svc.json");
    if !config.is_file() || !input.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let run = directory.path().join("run.toml");
    fs::write(
        &run,
        format!(
            "[meta]\nname = 'measurement run'\nloadcase = 'curb'\n\n[input.svc]\nfile = {:?}\n",
            input
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
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["case"]["name"], "measurement run");
    assert_eq!(report["case"]["loadcase"], "curb");
    assert_eq!(report["inputs"]["run"]["path"].as_str(), run.to_str());
}

#[test]
fn run_reports_missing_configured_input() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let directory = tempdir().unwrap();
    let run = directory.path().join("run.toml");
    fs::write(&run, "[input.svc]\nfile = 'input/missing.json'\n").unwrap();

    let result = clab()
        .args([
            "analyze",
            examples.join("config.toml").to_str().unwrap(),
            run.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stderr).trim(),
        format!(
            "error: failed to load config/run '{}': input/missing.json cannot be found",
            run.display()
        )
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
    assert!(stderr.contains("input file cannot be found"));
    assert!(stderr.contains(input.to_str().unwrap()));
    assert!(!stderr.contains("H5Fopen"));
}

#[test]
fn analyzes_knc_measurement_hdf5_when_config_also_defines_svc() {
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
            "-d",
            "knc",
            "-t",
            "vertical",
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
    // The converted ACD is KnC data and the config defines a KnC vertical test,
    // so it is not mistaken for an unsupported SVC measurement HDF5.
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!stderr.contains("SVC analysis does not support"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("KnC Vertical"));
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
            "-d",
            "svc",
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
    assert!(String::from_utf8_lossy(&result.stdout).contains("\nSVC\nrunning 29 metrics\n"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("KnC"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("warning:"));
    for name in ["report.json", "metrics.csv", "svc.json"] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let svc: Value =
        serde_json::from_str(&fs::read_to_string(output.join("svc.json")).unwrap()).unwrap();
    assert_eq!(svc["inputs"]["run"], report["inputs"]["run"]);
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
            "-d",
            "svc",
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
            "-d",
            "svc",
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
    assert!(String::from_utf8_lossy(&result.stdout).contains("\nSVC\nrunning 29 metrics\n"));
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
    let verbose_stderr = String::from_utf8_lossy(&verbose.stderr);
    assert!(verbose_stderr.contains("no HDF5 unit metadata"));
    assert!(verbose_stderr.contains("warning: svc: simulation signal"));
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
            "-d",
            "svc",
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
    let static_input = examples.join("input/example-simulation-knc-static.res");
    if !config.is_file() || !svc.is_file() || !vertical.is_file() || !static_input.is_file() {
        return;
    }
    let directory = tempdir().unwrap();
    let vertical_simulation = directory.path().join("vertical.h5");
    let result = clab()
        .args([
            "convert",
            vertical.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "vertical",
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
    let static_simulation = directory.path().join("static.h5");
    let result = clab()
        .args([
            "convert",
            static_input.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "static",
            "-o",
            static_simulation.to_str().unwrap(),
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
            "-d",
            "svc",
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
            "[meta]\nname = 'two-up run'\nloadcase = 'two-up'\n\n[input.svc]\nfile = {:?}\n\n[input.knc.vertical]\nfile = {:?}\n\n[input.knc.static]\nfile = {:?}\n",
            svc_simulation, vertical_simulation, static_simulation
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
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("\nSVC\nrunning 29 metrics\n"));
    // Both domains render through the same `domain[::test]::metric ... value unit` row shape.
    assert!(stdout.contains("  svc::total_mass_kg ... "));
    assert!(stdout.contains("  knc::vertical::suspension_rate_front_left_n_per_mm ... "));
    for name in [
        "report.json",
        "metrics.csv",
        "svc.json",
        "knc-vertical.json",
    ] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    let pdf = fs::read(output.join("plots/knc/vertical/knc_vertical_wcz_vs_wcy.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"].as_array().unwrap().len(), 2);
    assert_eq!(report["case"]["name"], "two-up run");
    assert_eq!(report["case"]["loadcase"], "two-up");
    for name in ["svc.json", "knc-vertical.json"] {
        let detail: Value =
            serde_json::from_str(&fs::read_to_string(output.join(name)).unwrap()).unwrap();
        assert_eq!(detail["case"], report["case"]);
        assert_eq!(detail["inputs"]["run"], report["inputs"]["run"]);
    }
}

#[test]
fn analyze_routes_hdf5_by_declared_domain() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let source = examples.join("input/example-measurement-knc-vertical.acd");
    let directory = tempdir().unwrap();
    let declared = directory.path().join("declared.h5");
    let converted = clab()
        .args([
            "convert",
            source.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "vertical",
            "-o",
        ])
        .arg(&declared)
        .output()
        .unwrap();
    assert!(converted.status.success());

    let result = clab()
        .args([
            "analyze",
            config.to_str().unwrap(),
            declared.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("KnC Vertical"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("schema version"));

    // The same data declared as svc now fails at convert time: the testtype
    // contradicts the declared domain, so nothing is written.
    let wrong = directory.path().join("wrong.h5");
    let converted = clab()
        .args(["convert", source.to_str().unwrap(), "-d", "svc", "-o"])
        .arg(&wrong)
        .output()
        .unwrap();
    assert!(!converted.status.success());
    assert!(String::from_utf8_lossy(&converted.stderr).contains("indicates knc"));
    assert!(!wrong.exists());

    // Data with no testtype metadata declares nothing to contradict, so
    // convert allows `-d svc` through; analyze must still reject the
    // result, since measurement HDF5 is never a valid SVC input.
    let no_testtype = directory.path().join("no-testtype.acd");
    fs::write(
        &no_testtype,
        "FileInfo: ASCII3\nTime\tWheel\tstring\tDescription\n0\t10\tfloat\tOffset\ntime\ttravel\tstring\tName\ns\tmm\tstring\tUnits\nNaN\tNaN\tstring\tKPoly\n0\t1\n",
    )
    .unwrap();
    let converted = clab()
        .args(["convert", no_testtype.to_str().unwrap(), "-d", "svc", "-o"])
        .arg(&wrong)
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    let result = clab()
        .args(["analyze", config.to_str().unwrap(), wrong.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("SVC analysis does not support measurement HDF5")
    );
}
