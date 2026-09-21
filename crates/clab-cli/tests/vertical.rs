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
    scalar(&file, "format_version", 3_u32);
    scalar(&file, "sample_count", 5_u64);
    scalar(&file, "signal_count", 16_u64);
    string(&file, "source_file", "fixture.res");
    let results = file.create_group("results").unwrap();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        let group = results.create_group(corner).unwrap();
        string(&group, "source_path", corner);
        group
            .new_dataset::<f64>()
            .shape((5, 4))
            .create("values")
            .unwrap()
            .write_raw(&[
                0., 100., 200., 300., -1., 98., 197., 299., 0., 100., 200., 300., 1., 102., 203.,
                301., 1., 102., 203., 301.,
            ])
            .unwrap();
        let strings = |values: &[&str]| {
            values
                .iter()
                .map(|v| VarLenUnicode::from_str(v).unwrap())
                .collect::<Vec<_>>()
        };
        group
            .new_dataset_builder()
            .with_data(&strings(&["travel", "x", "y", "z"]))
            .create("component_names")
            .unwrap();
        group
            .new_dataset_builder()
            .with_data(&strings(&["mm", "mm", "mm", "mm"]))
            .create("units")
            .unwrap();
        group
            .new_dataset_builder()
            .with_data(&[1_u64, 2, 3, 4])
            .create("adams_ids")
            .unwrap();
    }
    drop(results);
    file.close().unwrap();
}
fn channel(channels: &Group, name: &str, values: &[f64], offset: f64) {
    let group = channels.create_group(name).unwrap();
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
    scalar(&file, "format_version", 1_u32);
    scalar(&file, "sample_count", 6_u64);
    scalar(&file, "channel_count", 16_u64);
    string(&file, "source_kind", "measurement");
    string(&file, "source_file", "fixture.acd");
    let measurements = file.create_group("measurements").unwrap();
    measurements
        .new_dataset_builder()
        .with_data(&[0.; 6])
        .create("sample_axis")
        .unwrap();
    let channels = measurements.create_group("channels").unwrap();
    for prefix in ["lf", "rf", "lr", "rr"] {
        channel(
            &channels,
            &format!("{prefix}slope"),
            &[2., 2., 2., 3., 3., 3.],
            0.,
        );
        channel(
            &channels,
            &format!("{prefix}wcx"),
            &[98., 100., 102., 102., 100., 98.],
            100.,
        );
        channel(
            &channels,
            &format!("{prefix}wcy"),
            &[197., 200., 203., 203., 200., 197.],
            200.,
        );
        channel(
            &channels,
            &format!("{prefix}wcz"),
            &[299., 300., 301., 301., 300., 299.],
            300.,
        );
    }
    let info = measurements.create_group("test_info").unwrap();
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
    drop(channels);
    drop(measurements);
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
    assert!(String::from_utf8_lossy(&success.stderr).contains("no static_simulation"));
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
