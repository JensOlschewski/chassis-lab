use std::{fs, io::Write, process::Command, str::FromStr};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use serde_json::Value;
use tempfile::tempdir;

fn string(location: &Location, name: &str, value: &str) {
    location
        .new_attr::<VarLenUnicode>()
        .shape(())
        .create(name)
        .unwrap()
        .write_scalar(&VarLenUnicode::from_str(value).unwrap())
        .unwrap();
}
fn scalar<T: hdf5::H5Type>(location: &Location, name: &str, value: T) {
    location
        .new_attr::<T>()
        .create(name)
        .unwrap()
        .write_scalar(&value)
        .unwrap();
}
fn simulation(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    scalar(&file, "schema_version", 2_u32);
    scalar(&file, "sample_count", 5_u64);
    scalar(&file, "signal_count", 16_u64);
    string(&file, "source_kind", "simulation");
    string(&file, "domain", "knc");
    string(&file, "test", "vertical");
    string(&file, "source_file", "fixture.res");
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "sample_index");
    string(&axis, "unit", "");
    axis.new_dataset_builder()
        .with_data(&[0_u64, 1, 2, 3, 4])
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        for (component, values, id) in [
            ("travel", [0., -1., 0., 1., 1.], 1_u64),
            ("x", [100., 98., 100., 102., 102.], 2),
            ("y", [200., 197., 200., 203., 203.], 3),
            ("z", [300., 299., 300., 301., 301.], 4),
        ] {
            let group = signals
                .create_group(&format!("{corner}_{component}"))
                .unwrap();
            group
                .new_dataset_builder()
                .with_data(&values)
                .create("values")
                .unwrap();
            string(&group, "original_name", component);
            string(&group, "unit", "mm");
            string(&group, "source_path", corner);
            string(&group, "component", component);
            scalar(&group, "adams_id", id);
        }
    }
    drop(signals);
    file.close().unwrap();
}

fn clear_simulation_units(path: &std::path::Path) {
    let staging = path.with_extension("units.h5");
    fs::copy(path, &staging).unwrap();
    let file = File::open_rw(&staging).unwrap();
    let signals = file.group("signals").unwrap();
    for name in signals.member_names().unwrap() {
        let group = signals.group(&name).unwrap();
        group
            .attr("unit")
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str("").unwrap())
            .unwrap();
        drop(group);
    }
    drop(signals);
    file.close().unwrap();
    fs::copy(&staging, path).unwrap();
    fs::remove_file(staging).unwrap();
}
fn channel(signals: &Group, name: &str, values: &[f64], offset: f64) {
    let group = signals.create_group(name).unwrap();
    group
        .new_dataset_builder()
        .with_data(values)
        .create("values")
        .unwrap();
    string(&group, "original_name", name);
    string(&group, "description", name);
    let unit = if name.ends_with("slope") {
        ""
    } else if name.ends_with("steer") || name.ends_with("cam") {
        "deg"
    } else if name.ends_with("fz") {
        "N"
    } else {
        "mm"
    };
    string(&group, "unit", unit);
    scalar(&group, "offset", offset);
    scalar(&group, "column", 0_u64);
}
fn measurement(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    scalar(&file, "schema_version", 2_u32);
    scalar(&file, "sample_count", 6_u64);
    scalar(&file, "signal_count", 36_u64);
    string(&file, "source_kind", "measurement");
    string(&file, "domain", "knc");
    string(&file, "test", "vertical");
    string(&file, "source_file", "fixture.acd");
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "time");
    string(&axis, "unit", "s");
    axis.new_dataset_builder()
        .with_data(&[0.; 6])
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    for prefix in ["lf", "rf", "lr", "rr"] {
        channel(
            &signals,
            &format!("{prefix}slope"),
            &[2., 2., 2., 3., 3., 3.],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}wcx"),
            &[98., 100., 102., 102., 100., 98.],
            100.,
        );
        channel(
            &signals,
            &format!("{prefix}wcy"),
            &[197., 200., 203., 203., 200., 197.],
            200.,
        );
        channel(
            &signals,
            &format!("{prefix}wcz"),
            &[299., 300., 301., 301., 300., 299.],
            300.,
        );
        channel(
            &signals,
            &format!("{prefix}steer"),
            &[-0.02, 0., 0.02, 0.02, 0., -0.02],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}cam"),
            &[0.03, 0., -0.03, -0.03, 0., 0.03],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}fz"),
            &[2970., 3000., 3030., 3030., 3000., 2970.],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}pz"),
            &[-1.1, 0., 1.1, 1.1, 0., -1.1],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}tdef"),
            &[16.9, 17., 17.1, 17.1, 17., 16.9],
            17.,
        );
    }
    let metadata = file.create_group("measurement").unwrap();
    let info = metadata.create_group("test_info").unwrap();
    for (name, value) in [
        ("testtype", "vert"),
        ("brakeson", "1"),
        ("engineon", "0"),
        ("farbon", "1"),
        ("rarbon", "unconfigured-and-ignored"),
    ] {
        let group = info.create_group(name).unwrap();
        for key in ["name", "description", "data_type", "unit"] {
            string(&group, key, "");
        }
        string(&group, "value", value);
    }
    drop(info);
    drop(signals);
    drop(axis);
    drop(metadata);
    file.close().unwrap();
}
fn closed_fixture(path: &std::path::Path, write: fn(&std::path::Path)) {
    let staging = path.with_extension("fixture.h5");
    write(&staging);
    fs::copy(&staging, path).unwrap();
    fs::remove_file(staging).unwrap();
}
fn config(path: &std::path::Path, omit: bool) {
    let mut signals = String::new();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        for (name, component) in [
            ("travel", "travel"),
            ("wheel_center_x", "x"),
            ("wheel_center_y", "y"),
            ("wheel_center_z", "z"),
        ] {
            if omit && corner == "front_left" && name == "wheel_center_x" {
                continue;
            }
            signals.push_str(&format!("[mappings.knc.vertical.signals.{corner}_{name}]\nentity = \"{corner}\"\ncomponent = \"{component}\"\nunit = \"mm\"\n"));
        }
    }
    fs::write(path,format!("[meta]\nname=\"fixture\"\nloadcase=\"L\"\n[physics]\ngravity=9.80665\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n{signals}" )).unwrap();
}

#[test]
fn analyzes_normalized_hdf5_as_curve_bundle() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&measurement_path, measurement);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let output = directory.path().join("reports");
    let json = output.join("knc-vertical.json");
    let svg = output.join("plots/knc/vertical/knc_vertical_wcz_vs_wcy.pdf");
    let recession = output.join("plots/knc/vertical/knc_vertical_recession_angle.pdf");
    let success = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(json.is_file() && svg.is_file() && recession.is_file());
    assert!(fs::read(recession).unwrap().starts_with(b"%PDF"));
    let report = fs::read_to_string(output.join("report.json")).unwrap();
    assert!(report.contains("no static_simulation"));
    assert!(report.contains("\"method\": \"none\""));
    let stderr = String::from_utf8_lossy(&success.stderr);
    assert!(stderr.contains("analysis warning"));
    assert!(
        output
            .join("plots/knc/vertical/knc_vertical_wcz_vs_wcx.pdf")
            .is_file()
    );
    assert!(!output.join("knc-vertical.pdf").exists());
    assert!(fs::read(svg).unwrap().starts_with(b"%PDF"));
    assert!(
        fs::read_to_string(&json)
            .unwrap()
            .contains("wheel_center_z")
    );
    let rejected = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    let missing = directory.path().join("missing.toml");
    config(&missing, true);
    let missing_result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            missing.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(missing_result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_result.stderr).contains("mapping missing"));
}

#[test]
fn analyzes_measurement_vertical_gradient_metrics() {
    let directory = tempdir().unwrap();
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&measurement_path, measurement);
    // Explicit gradient points and default limited to the ±1 mm window.
    for gradient in ["gradient_points_mm=0.5\n", ""] {
        assert_measurement_metrics(directory.path(), &measurement_path, gradient);
    }
}

fn assert_measurement_metrics(
    directory: &std::path::Path,
    measurement_path: &std::path::Path,
    gradient: &str,
) {
    let config_path = directory.join("case.toml");
    config(&config_path, false);
    let text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("step_mm=1\n", "step_mm=1\nplot_branches='all'\n");
    fs::write(
        &config_path,
        format!("{text}\n[setup.knc.vertical.metric]\n{gradient}direction='any'\nindex=1\n"),
    )
    .unwrap();
    let output = directory.join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            measurement_path.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
            "--force",
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("KnC Vertical\nrunning 24 metrics"));
    let stderr = String::from_utf8_lossy(&result.stderr);
    if gradient.is_empty() {
        // Every other key is set, so only the gradient default is reported.
        assert_eq!(stderr.matches("warning:").count(), 1, "{stderr}");
        assert!(stderr.contains(
            "warning: KnC vertical config key 'gradient_points_mm' missing in [setup.knc.metric]; \
             using default 10 mm (limited to travel window: 1 mm)"
        ));
        return;
    }
    assert!(!stderr.contains("warning:"), "{stderr}");
    let detail: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-vertical.json")).unwrap())
            .unwrap();
    let value = |name: &str| {
        detail["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["metric"] == name)
            .unwrap_or_else(|| panic!("missing metric {name}"))["value"]
            .as_f64()
            .unwrap()
    };
    let close = |actual: f64, expected: f64| assert!((actual - expected).abs() < 1e-9, "{actual}");
    close(value("bump_steer_front_left_deg_per_mm"), 0.02);
    close(value("bump_steer_front_right_deg_per_mm"), -0.02);
    close(value("bump_camber_front_left_deg_per_mm"), -0.03);
    close(
        value("wheel_recession_angle_front_left_deg"),
        (-2.0_f64).atan().to_degrees(),
    );
    close(value("suspension_rate_rear_right_n_per_mm"), 30.0);
    close(value("ride_rate_rear_right_n_per_mm"), 30.0 / 1.1);
    close(value("tire_rate_rear_right_n_per_mm"), 300.0);
    let csv = fs::read_to_string(output.join("metrics.csv")).unwrap();
    assert!(csv.starts_with("run,domain,test,metric,unit,value\n"));
    assert!(csv.contains("fixture,knc,vertical,suspension_rate_rear_right_n_per_mm,N/mm,30"));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"][0]["metric_count"], 24);
}

#[test]
fn skips_vertical_metrics_without_static_normalization() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    closed_fixture(&simulation_path, simulation);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let text = fs::read_to_string(&config_path)
        .unwrap()
        .replace("step_mm=1\n", "step_mm=1\nmetric_gradient_points_mm=0.5\n");
    fs::write(&config_path, text).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("KnC Vertical\nrunning 0 metrics"));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("vertical metrics require static_simulation normalization")
    );
}

#[test]
fn analyzes_vertical_without_output() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    closed_fixture(&simulation_path, simulation);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn analyzes_simulation_with_missing_units_and_warns() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    closed_fixture(&simulation_path, simulation);
    clear_simulation_units(&simulation_path);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let output = directory.path().join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
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
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("analysis warnings; details in"));
    assert!(!stderr.contains("no HDF5 unit metadata"));
    let report = fs::read_to_string(output.join("report.json")).unwrap();
    assert!(report.contains("no HDF5 unit metadata"));

    let verbose = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(verbose.status.success());
    assert!(String::from_utf8_lossy(&verbose.stderr).contains("no HDF5 unit metadata"));
}

#[test]
fn run_simulation_static_normalizes_and_reports_available_metrics() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let static_path = directory.path().join("static.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&static_path, simulation);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let run = directory.path().join("run.toml");
    fs::write(
        &run,
        format!(
            "[input.knc.static]\nfile = \"{}\"\n[input.knc.vertical]\nfile = \"{}\"\n",
            static_path.display(),
            simulation_path.display()
        ),
    )
    .unwrap();
    let output = directory.path().join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
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
    assert!(!String::from_utf8_lossy(&result.stderr).contains("no static_simulation"));
    let report = fs::read_to_string(output.join("report.json")).unwrap();
    assert!(report.contains("drop_initial_sample_and_subtract_final_sample"));
    assert!(!report.contains("no static_simulation"));
    let detail: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-vertical.json")).unwrap())
            .unwrap();
    let front_left_x = detail["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|curve| curve["corner"] == "front_left" && curve["signal"] == "wheel_center_x")
        .unwrap();
    assert_eq!(
        front_left_x["values"],
        serde_json::json!([-4.0, -2.0, 0.0, 0.0])
    );
}

/// Measurement config with the first sweep selected, so every metric exists.
fn comparison_config(path: &std::path::Path, tolerance: &str) {
    config(path, false);
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(
            format!("\n[setup.knc.vertical.branch]\ndirection='any'\nindex=1\n{tolerance}")
                .as_bytes(),
        )
        .unwrap();
}

const EXPECTED_METRICS: [(&str, &str); 6] = [
    ("bump_camber", "deg_per_mm"),
    ("bump_steer", "deg_per_mm"),
    ("ride_rate", "n_per_mm"),
    ("suspension_rate", "n_per_mm"),
    ("tire_rate", "n_per_mm"),
    ("wheel_recession_angle", "deg"),
];

#[test]
fn compares_only_gradient_metrics_with_overlays() {
    let directory = tempdir().unwrap();
    let baseline_path = directory.path().join("baseline.h5");
    let candidate_path = directory.path().join("candidate.h5");
    closed_fixture(&baseline_path, measurement);
    closed_fixture(&candidate_path, measurement);
    let config_path = directory.path().join("case.toml");
    comparison_config(&config_path, "");
    let output = directory.path().join("comparison");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "compare",
            config_path.to_str().unwrap(),
            baseline_path.to_str().unwrap(),
            candidate_path.to_str().unwrap(),
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
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["candidates"][0]["domains"][0]["domain"], "knc");
    assert_eq!(report["candidates"][0]["domains"][0]["status"], "compared");
    let mut expected = Vec::new();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        for (metric, unit) in EXPECTED_METRICS {
            expected.push(format!("{metric}_{corner}_{unit}"));
        }
    }
    expected.sort();
    let metrics = report["candidates"][0]["metrics"].as_array().unwrap();
    assert_eq!(
        metrics
            .iter()
            .map(|metric| metric["metric"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        metrics
            .iter()
            .all(|metric| metric["status"] == "informational")
    );
    assert_eq!(report["status"], "informational");
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("KnC Vertical\nbaseline:"));
    assert!(stdout.contains("knc::vertical::bump_steer_front_left_deg_per_mm ... (baseline"));
    assert!(!stdout.contains("wheel_center_x_mm"));
    let svg = output.join("plots/knc/vertical/comparison/knc_vertical_wcz_vs_wcy.pdf");
    assert!(fs::read(svg).unwrap().starts_with(b"%PDF"));
    assert!(output.join("metrics.csv").is_file());
}

#[test]
fn compares_direct_vertical_inputs_without_output() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&measurement_path, measurement);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "compare",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            measurement_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!directory.path().join("plots").exists());
}

#[test]
fn compare_config_tolerances_apply_knc_vertical_section() {
    let directory = tempdir().unwrap();
    let baseline_path = directory.path().join("baseline.h5");
    let candidate_path = directory.path().join("candidate.h5");
    closed_fixture(&baseline_path, measurement);
    closed_fixture(&candidate_path, measurement);
    let config_path = directory.path().join("case.toml");
    comparison_config(
        &config_path,
        "[validation.knc.vertical.tolerances.absolute]\nbump_steer_front_left_deg_per_mm=0.001\n",
    );
    let output = directory.path().join("comparison");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "compare",
            config_path.to_str().unwrap(),
            baseline_path.to_str().unwrap(),
            candidate_path.to_str().unwrap(),
            "--tolerances",
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
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert!(
        report["candidates"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |metric| metric["metric"] == "bump_steer_front_left_deg_per_mm"
                    && metric["absolute_tolerance"] == 0.001
                    && metric["status"] == "passed"
            )
    );
}
