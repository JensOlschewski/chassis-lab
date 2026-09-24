use std::{fs, process::Command, str::FromStr};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
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
    scalar(&file, "schema_version", 1_u32);
    scalar(&file, "sample_count", 5_u64);
    scalar(&file, "signal_count", 16_u64);
    string(&file, "source_kind", "simulation");
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
    string(
        &group,
        "unit",
        if name.ends_with("slope") { "" } else { "mm" },
    );
    scalar(&group, "offset", offset);
    scalar(&group, "column", 0_u64);
}
fn measurement(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    scalar(&file, "schema_version", 1_u32);
    scalar(&file, "sample_count", 6_u64);
    scalar(&file, "signal_count", 16_u64);
    string(&file, "source_kind", "measurement");
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
    let svg = output.join("plots/knc/vertical/wheel-center-z-vs-wheel-center-y.svg");
    let success = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(json.is_file() && svg.is_file());
    let report = fs::read_to_string(output.join("report.json")).unwrap();
    assert!(report.contains("no static_simulation"));
    assert!(report.contains("\"method\": \"none\""));
    let stderr = String::from_utf8_lossy(&success.stderr);
    assert!(stderr.contains("1 analysis warning; details in"));
    assert!(!stderr.contains("no static_simulation"));
    assert!(
        output
            .join("plots/knc/vertical/wheel-center-y-vs-wheel-center-x.svg")
            .is_file()
    );
    assert!(
        output
            .join("plots/knc/vertical/wheel-center-z-vs-wheel-center-x.svg")
            .is_file()
    );
    assert!(!output.join("knc-vertical.svg").exists());
    let svg_text = fs::read_to_string(svg).unwrap();
    assert!(svg_text.contains("Simulation"));
    assert!(svg_text.contains("Front left"));
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
            "-o",
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
fn run_simulation_static_normalizes_without_warning() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let static_path = directory.path().join("static.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&static_path, simulation);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let run = directory.path().join("run.toml");
    fs::write(&run, format!("[input.knc.vertical.simulation]\nfile = \"{}\"\n[input.knc.vertical.static_simulation]\nfile = \"{}\"\n", simulation_path.display(), static_path.display())).unwrap();
    let output = directory.path().join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
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
    assert!(!String::from_utf8_lossy(&result.stderr).contains("warning:"));
    let report = fs::read_to_string(output.join("report.json")).unwrap();
    assert!(report.contains("subtract_final_sample"));
    assert!(!report.contains("no static_simulation"));
}
