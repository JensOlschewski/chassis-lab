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
fn config(path: &std::path::Path, measurement: &str, omit: bool) {
    let mut signals = String::new();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        for (name, component) in [
            ("travel", "travel"),
            ("wheel_center_x", "x"),
            ("wheel_center_y", "y"),
            ("wheel_center_z", "z"),
        ] {
            if omit && corner == "front_left" && name == "travel" {
                continue;
            }
            signals.push_str(&format!("[vertical.signals.{corner}_{name}]\nentity = \"{corner}\"\ncomponent = \"{component}\"\nunit = \"mm\"\n"));
        }
    }
    fs::write(path,format!("[case]\nname=\"fixture\"\nloadcase=\"L\"\n[vertical.inputs]\nmeasurement_hdf5=\"{measurement}\"\nsimulation_hdf5=\"simulation.h5\"\n[vertical.setup]\ncode=\"b1e0f1\"\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n{signals}" )).unwrap();
}
fn command(config: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.args(["validate", "-t", "vertical"]).arg(config);
    command
}

#[test]
fn validates_normalized_hdf5_with_embedded_python_and_safe_outputs() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&measurement_path, measurement);
    let config_path = directory.path().join("case.toml");
    config(&config_path, "measurement.h5", false);
    let check = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args(["check", config_path.to_str().unwrap()])
        .env("PYTHONHOME", "/path/that/does/not/exist")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(!directory.path().join("reports").exists());
    let vertical_output = directory.path().join("vertical-reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "validate",
            config_path.to_str().unwrap(),
            "-t",
            "vertical",
            "--output-dir",
            vertical_output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(vertical_output.join("knc-vertical.json").is_file());
    assert!(!vertical_output.join("svc.json").exists());

    let output = directory.path().join("reports");
    let json = output.join("knc-vertical.json");
    let csv = output.join("knc-vertical.csv");
    let svg = output.join("knc-vertical.svg");
    let success = command(&config_path)
        .arg("--output-dir")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(json.is_file() && csv.is_file() && svg.is_file());
    assert!(
        fs::read_to_string(&json)
            .unwrap()
            .contains("measurement_hdf5")
    );
    let rejected = command(&config_path)
        .arg("--output-dir")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("already exists"));
    assert!(
        command(&config_path)
            .arg("--output-dir")
            .arg(&output)
            .arg("--force")
            .output()
            .unwrap()
            .status
            .success()
    );
    let raw = directory.path().join("raw.toml");
    config(&raw, "measurement.acd", false);
    fs::write(
        &raw,
        fs::read_to_string(&raw)
            .unwrap()
            .replace("measurement_hdf5", "measurement_acd"),
    )
    .unwrap();
    fs::write(directory.path().join("measurement.acd"), "raw").unwrap();
    let raw_result = command(&raw).output().unwrap();
    assert_eq!(raw_result.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&raw_result.stderr)
            .contains("unknown [vertical.inputs] key: measurement_acd")
    );
    let missing = directory.path().join("missing.toml");
    config(&missing, "measurement.h5", true);
    let missing_result = command(&missing).output().unwrap();
    assert_eq!(missing_result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_result.stderr).contains("mapping missing"));
}

#[test]
fn defaults_to_all_configured_domains() {
    let directory = tempdir().unwrap();
    closed_fixture(&directory.path().join("simulation.h5"), simulation);
    closed_fixture(&directory.path().join("measurement.h5"), measurement);
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let svc_simulation = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/example-simulation-svc.json");
    let svc = fs::read_to_string(examples.join("example-config.toml"))
        .unwrap()
        .replace(
            "\"example-signals.toml\"",
            &format!("\"{}\"", examples.join("example-signals.toml").display()),
        )
        .replace(
            "\"example-measurement-svc.json\"",
            &format!(
                "\"{}\"",
                examples.join("example-measurement-svc.json").display()
            ),
        )
        .replace(
            "\"example-svc-tolerances.toml\"",
            &format!(
                "\"{}\"",
                examples.join("example-svc-tolerances.toml").display()
            ),
        )
        .replace(
            "\"example-simulation-svc.json\"",
            &format!("\"{}\"", svc_simulation.display()),
        )
        .replace(
            "\"example-vertical-signals.toml\"",
            &format!(
                "\"{}\"",
                examples.join("example-vertical-signals.toml").display()
            ),
        )
        .replace(
            "\"example-simulation-vertical.h5\"",
            &format!("\"{}\"", directory.path().join("simulation.h5").display()),
        )
        .replace(
            "\"example-measurement-vertical.h5\"",
            &format!("\"{}\"", directory.path().join("measurement.h5").display()),
        )
        .replace("brakes_applied = false", "brakes_applied = true")
        .replace("rear_arb_connected = true", "");
    let case = directory.path().join("case.toml");
    fs::write(&case, svc).unwrap();
    let vertical_output = directory.path().join("vertical-reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "validate",
            case.to_str().unwrap(),
            "-t",
            "vertical",
            "--output-dir",
            vertical_output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(vertical_output.join("knc-vertical.json").is_file());
    assert!(!vertical_output.join("svc.json").exists());
    let output = directory.path().join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "validate",
            case.to_str().unwrap(),
            "--output-dir",
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
        "svc.json",
        "svc.csv",
        "knc-vertical.json",
        "knc-vertical.csv",
        "knc-vertical.svg",
    ] {
        assert!(output.join(name).is_file(), "missing {name}");
    }
}
