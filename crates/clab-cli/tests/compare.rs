use std::{fs, path::Path, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn clab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_clab"))
}

/// Shared fixture (`tests/fixtures/svc-simulation-mapping.toml`): SVC
/// simulation mapping for the 24 canonical WHEEL_LOADS + GEOMETRY signals,
/// matching `write_simulation_json`'s entity/component names below. Read
/// from a file (not duplicated inline) so it has one definition shared
/// with `analyze.rs`, and renaming or restructuring `examples/` never
/// breaks a test whose only need was "some valid SVC mapping".
fn svc_simulation_mapping_toml() -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/svc-simulation-mapping.toml"),
    )
    .unwrap()
}

fn config_with_tolerance(directory: &Path, tolerance: f64) -> std::path::PathBuf {
    let tolerances = directory.join("tolerances.toml");
    fs::write(
        &tolerances,
        format!("[svc.validation.tolerances.absolute]\ntotal_mass_kg = {tolerance}\n"),
    )
    .unwrap();
    let config = directory.join("config.toml");
    fs::write(
        &config,
        format!(
            "[physics]\ngravity = 9.80665\n{}\n[compare]\nfile = \"tolerances.toml\"\n",
            svc_simulation_mapping_toml()
        ),
    )
    .unwrap();
    config
}

fn write_simulation_json(path: &Path, front_left_load: f64) {
    let component = |name: &str, unit: &str, value: f64| serde_json::json!({"name": name, "adams_id": 1, "units": unit, "values": [value]});
    let simulation = serde_json::json!({
        "schema_version": 1, "source_kind": "simulation", "source_file": "synthetic.res",
        "sample_count": 1, "values_per_sample": 25,
        "entities": [
            {"name": "example.wheel_loads", "components": [component("front_left", "newton", front_left_load), component("front_right", "newton", 4000.0), component("rear_left", "newton", 4000.0), component("rear_right", "newton", 4000.0)]},
            {"name": "example.geometry", "components": [component("wheelbase", "mm", 2800.0), component("front_track", "mm", 1600.0), component("rear_track", "mm", 1600.0), component("cg_height", "mm", 500.0), component("inertia_xx", "kg*mm^2", 1.0), component("inertia_yy", "kg*mm^2", 1.0), component("inertia_zz", "kg*mm^2", 1.0), component("front_left_wheel_center_x", "mm", 0.0), component("gyro_x", "mm", 0.0)]},
            {"name": "example.ride_heights", "components": [component("front_left", "mm", 100.0), component("front_right", "mm", 100.0), component("rear_left", "mm", 100.0), component("rear_right", "mm", 100.0)]},
            {"name": "example.toe_camber", "components": [component("front_left_toe", "deg", 0.0), component("front_right_toe", "deg", 0.0), component("rear_left_toe", "deg", 0.0), component("rear_right_toe", "deg", 0.0), component("front_left_camber", "deg", 0.0), component("front_right_camber", "deg", 0.0), component("rear_left_camber", "deg", 0.0), component("rear_right_camber", "deg", 0.0)]}
        ]
    });
    fs::write(path, serde_json::to_string(&simulation).unwrap()).unwrap();
}

#[test]
fn compares_svc_inputs_and_protects_reports_and_inputs() {
    let directory = tempdir().unwrap();
    let config = config_with_tolerance(directory.path(), 0.0001);
    let simulation = directory.path().join("simulation.json");
    let candidate = directory.path().join("candidate.json");
    write_simulation_json(&simulation, 4000.0);
    write_simulation_json(&candidate, 4200.0);
    let output = directory.path().join("reports");
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
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
    assert!(stdout.starts_with("\nSVC\nbaseline:"));
    assert!(stdout.contains("candidate_1:"));
    assert!(stdout.contains("svc::total_mass_kg ... (baseline"));
    assert!(!stdout.contains(" ... ok") && !stdout.contains(" ... failed"));
    assert!(
        stdout
            .contains("comparison result: informational. 29 compared; 0 metric missing; 29 total")
    );
    assert!(
        !stdout.contains("\x1b["),
        "piped output must not contain ANSI"
    );
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
        .map(|metric| stdout.find(&format!("svc::{metric}")).unwrap())
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    for name in ["report.json", "metrics.csv", "svc.json"] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["command"], "compare");
    assert_eq!(report["status"], "informational");
    assert_eq!(report["baseline"]["case"]["name"], "undefined");
    assert_eq!(report["baseline"]["case"]["loadcase"], "undefined");
    assert_eq!(report["candidates"][0]["case"]["name"], "undefined");
    assert_eq!(report["candidates"][0]["case"]["loadcase"], "undefined");
    assert_eq!(
        report["candidates"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|metric| metric["metric"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected.as_slice()
    );
    let mut csv = csv::Reader::from_path(output.join("metrics.csv")).unwrap();
    assert_eq!(csv.headers().unwrap().get(0), Some("baseline"));
    let records = csv
        .records()
        .map(|record| record.unwrap())
        .collect::<Vec<_>>();
    assert!(
        records
            .iter()
            .all(|record| record.get(12) == Some("informational"))
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.get(4).unwrap())
            .collect::<Vec<_>>(),
        expected.as_slice()
    );
    let rejected = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    let protected_input = directory.path().join("report.json");
    fs::copy(&simulation, &protected_input).unwrap();
    let rejected = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            protected_input.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "-r",
            directory.path().to_str().unwrap(),
            "--force",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("must not replace an input"));
}

#[test]
fn compares_svc_json_and_returns_one_for_failed_tolerance() {
    let directory = tempdir().unwrap();
    let config = config_with_tolerance(directory.path(), 0.0001);
    let simulation = directory.path().join("simulation.json");
    let candidate = directory.path().join("candidate.json");
    write_simulation_json(&simulation, 4000.0);
    write_simulation_json(&candidate, 4200.0);
    let output = directory.path().join("failed-report");
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            simulation.to_str().unwrap(),
            candidate.to_str().unwrap(),
            "--tolerances",
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}\n{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["status"], "failed");
    assert!(
        report["candidates"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| metric["metric"] == "total_mass_kg" && metric["status"] == "failed")
    );
}

#[test]
fn aggregates_candidates_once_and_marks_missing_without_extra_sections() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let directory = tempdir().unwrap();
    let baseline = directory.path().join("baseline.json");
    let candidate_a = directory.path().join("candidate-a.json");
    let candidate_b = directory.path().join("candidate-b.json");
    write_simulation_json(&baseline, 4000.0);
    write_simulation_json(&candidate_a, 4100.0);
    write_simulation_json(&candidate_b, 4200.0);
    let output = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            baseline.to_str().unwrap(),
            candidate_a.to_str().unwrap(),
            candidate_b.to_str().unwrap(),
            "--tolerances",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("SVC\n").count(), 1);
    assert!(stdout.contains("candidate_1:"));
    assert!(stdout.contains("candidate_2:"));
    assert!(stdout.contains("candidate_1 "));
    assert!(stdout.contains("diff_1 "));
    assert!(stdout.contains("candidate_2 "));
    assert!(stdout.contains("diff_2 "));
    assert!(stdout.contains("FAILED FAILED"));
    assert!(stdout.starts_with('\n'));
}

#[test]
fn compares_svc_simulation_and_measurement_references() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let config = examples.join("config.toml");
    let measurement = examples.join("input/generated-measurement-svc.json");
    let directory = tempdir().unwrap();
    let simulation = directory.path().join("simulation.json");
    write_simulation_json(&simulation, 4000.0);
    let output = directory.path().join("comparison");
    let result = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            measurement.to_str().unwrap(),
            simulation.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_ne!(
        result.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let metrics = report["candidates"][0]["metrics"].as_array().unwrap();
    assert_eq!(metrics.len(), 29);
    assert_eq!(metrics[0]["metric"], "front_left_mass_kg");
    assert_eq!(metrics[28]["metric"], "inertia_zz_kg_mm2");
    assert!(metrics.iter().any(|metric| {
        metric["metric"] == "total_mass_kg"
            && metric["baseline_value"].is_number()
            && metric["candidate_value"].is_number()
            && metric["status"] != "missing"
    }));
    assert!(metrics.iter().any(|metric| {
        metric["metric"] == "camber_rear_right_deg"
            && metric["baseline_value"].is_null()
            && metric["candidate_value"].is_number()
            && metric["difference"].is_null()
            && metric["status"] == "missing"
    }));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("svc::camber_rear_right_deg ... missing (baseline missing; candidate_1 0.00000000; diff_1 missing)"));
    assert!(!stdout.contains("reference 0.00000000; diff 0.00000000)"));
}

#[test]
fn rejects_missing_compare_inputs_before_hdf5_dispatch() {
    let directory = tempdir().unwrap();
    let config = config_with_tolerance(directory.path(), 0.0001);
    let missing_hdf5 = directory.path().join("missing.h5");
    let direct = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            missing_hdf5.to_str().unwrap(),
            missing_hdf5.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(direct.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&direct.stderr);
    assert!(stderr.contains(&format!(
        "comparison input does not exist: {}",
        missing_hdf5.display()
    )));
    assert!(!stderr.contains("H5Fopen"));

    let run = directory.path().join("missing-input-run.toml");
    fs::write(&run, "[input.svc]\nfile = 'missing.h5'\n").unwrap();
    let manifest = clab()
        .args([
            "compare",
            config.to_str().unwrap(),
            run.to_str().unwrap(),
            run.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(manifest.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&manifest.stderr);
    assert!(stderr.contains(&format!(
        "comparison input does not exist: {}",
        directory.path().join("missing.h5").display()
    )));
    assert!(!stderr.contains("H5Fopen"));
}
