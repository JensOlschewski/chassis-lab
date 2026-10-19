use std::{fs, path::Path, process::Command, str::FromStr};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use serde_json::Value;
use tempfile::tempdir;

const CORNERS: [(&str, &str); 4] = [
    ("front_left", "lf"),
    ("front_right", "rf"),
    ("rear_left", "lr"),
    ("rear_right", "rr"),
];

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

fn header(file: &File, source_kind: &str, test: &str, samples: u64) {
    scalar(file, "schema_version", 2_u32);
    scalar(file, "sample_count", samples);
    string(file, "source_kind", source_kind);
    string(file, "domain", "knc");
    string(file, "test", test);
    string(file, "source_file", "fixture");
}

fn channel(signals: &Group, name: &str, values: &[f64], unit: &str, offset: f64) {
    let group = signals.create_group(name).unwrap();
    group
        .new_dataset_builder()
        .with_data(values)
        .create("values")
        .unwrap();
    string(&group, "original_name", name);
    string(&group, "description", name);
    string(&group, "unit", unit);
    scalar(&group, "offset", offset);
    scalar(&group, "column", 0_u64);
}

/// Body rolls +2 deg: left wheels rise, right wheels drop.
fn roll_measurement(path: &Path) {
    let file = File::create(path).unwrap();
    header(&file, "measurement", "roll", 3);
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "time");
    string(&axis, "unit", "s");
    axis.new_dataset_builder()
        .with_data(&[0., 1., 2.])
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    for (corner, prefix) in CORNERS {
        let side = if corner.ends_with("left") { 1. } else { -1. };
        let lateral = side * 750.;
        channel(&signals, &format!("{prefix}wcx"), &[0.; 3], "mm", 0.);
        channel(
            &signals,
            &format!("{prefix}wcy"),
            &[lateral; 3],
            "mm",
            lateral,
        );
        channel(
            &signals,
            &format!("{prefix}wcz"),
            &[300., 300. + side * 10., 300. + side * 20.],
            "mm",
            300.,
        );
        channel(
            &signals,
            &format!("{prefix}copy"),
            &[lateral, lateral + 1., lateral + 2.],
            "mm",
            lateral,
        );
    }
    for (name, unit, values) in [
        ("troll", "deg", [0., 1., 2.]),
        ("fsuspensionroll", "deg", [0., 0.9, 1.8]),
        ("fmx", "Nm", [0., 500., 1000.]),
        ("tmx", "Nm", [0., 900., 1800.]),
    ] {
        channel(&signals, name, &values, unit, 0.);
    }
    scalar(
        &file,
        "signal_count",
        signals.member_names().unwrap().len() as u64,
    );
    drop(signals);
    drop(axis);
    file.close().unwrap();
}

fn roll_simulation(path: &Path, test: &str) {
    let file = File::create(path).unwrap();
    header(&file, "simulation", test, 3);
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "sample_index");
    string(&axis, "unit", "");
    axis.new_dataset_builder()
        .with_data(&[0_u64, 1, 2])
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    let add = |entity: &str, component: &str, unit: &str, values: [f64; 3]| {
        let group = signals
            .create_group(&format!("{entity}_{component}"))
            .unwrap();
        group
            .new_dataset_builder()
            .with_data(&values)
            .create("values")
            .unwrap();
        string(&group, "original_name", component);
        string(&group, "unit", unit);
        string(&group, "source_path", entity);
        string(&group, "component", component);
        scalar(&group, "adams_id", 1_u64);
    };
    for (corner, _) in CORNERS {
        let side = if corner.ends_with("left") { 1. } else { -1. };
        add(corner, "travel", "mm", [0., side * 10., side * 20.]);
        add(corner, "x", "mm", [100.; 3]);
        add(corner, "y", "mm", [side * 750.; 3]);
        add(
            corner,
            "z",
            "mm",
            [300., 300. + side * 10., 300. + side * 20.],
        );
    }
    if test == "roll" {
        add("body", "roll", "deg", [0., 1., 2.]);
    }
    scalar(
        &file,
        "signal_count",
        signals.member_names().unwrap().len() as u64,
    );
    drop(signals);
    drop(axis);
    file.close().unwrap();
}

fn closed_fixture(path: &Path, write: impl Fn(&Path)) {
    let staging = path.with_extension("fixture.h5");
    write(&staging);
    fs::copy(&staging, path).unwrap();
    fs::remove_file(staging).unwrap();
}

/// `[setup.knc]` shared with `[setup.knc.vertical]` and `[setup.knc.roll]`
/// overrides; only the roll override may apply to roll.
fn config(path: &Path, roll_angle: bool) {
    let mut text = String::from(
        "[meta]\nname=\"fixture\"\nloadcase=\"L\"\n\
         [setup.knc.meta]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=false\n\
         [setup.knc.window]\nminimum_mm=-30\nmaximum_mm=30\nstep_mm=1\n\
         [setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n\
         [setup.knc.roll.metric]\ngradient_points_mm=5\n",
    );
    for (corner, prefix) in CORNERS {
        for (name, channel) in [
            ("wheel_center_x", "wcx"),
            ("wheel_center_y", "wcy"),
            ("wheel_center_z", "wcz"),
            ("center_of_pressure_y", "copy"),
        ] {
            text.push_str(&format!(
                "[mappings.measurement.knc.roll.signals.{corner}_{name}]\nentity=\"{prefix}{channel}\"\nunit=\"mm\"\n"
            ));
        }
        for (name, component) in [
            ("travel", "travel"),
            ("wheel_center_x", "x"),
            ("wheel_center_y", "y"),
            ("wheel_center_z", "z"),
        ] {
            text.push_str(&format!(
                "[mappings.simulation.knc.roll.signals.{corner}_{name}]\nentity=\"{corner}\"\ncomponent=\"{component}\"\nunit=\"mm\"\n"
            ));
        }
    }
    if roll_angle {
        text.push_str(
            "[mappings.measurement.knc.roll.signals.total_roll_angle]\nentity=\"troll\"\nunit=\"deg\"\n\
             [mappings.simulation.knc.roll.signals.total_roll_angle]\nentity=\"body\"\ncomponent=\"roll\"\nunit=\"deg\"\n",
        );
    }
    for (name, entity, unit) in [
        ("front_suspension_roll_angle", "fsuspensionroll", "deg"),
        ("front_roll_moment", "fmx", "Nm"),
        ("total_roll_moment", "tmx", "Nm"),
        // Mapped but absent from the file: skipped like any optional signal.
        ("rear_roll_moment", "rmx", "Nm"),
    ] {
        text.push_str(&format!(
            "[mappings.measurement.knc.roll.signals.{name}]\nentity=\"{entity}\"\nunit=\"{unit}\"\n"
        ));
    }
    fs::write(path, text).unwrap();
}

fn clab(args: &[&Path]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command
        .arg("analyze")
        .args(args.iter().map(|path| path.as_os_str()));
    command.output().unwrap()
}

fn curve<'a>(detail: &'a Value, corner: &str, signal: &str) -> &'a Value {
    detail["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|curve| curve["corner"] == corner && curve["signal"] == signal)
        .unwrap_or_else(|| panic!("missing curve {corner}/{signal}"))
}

fn values(curve: &Value) -> Vec<f64> {
    curve["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect()
}

#[test]
fn analyzes_roll_measurement_as_curve_bundle() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("roll.h5");
    closed_fixture(&input, roll_measurement);
    let config_path = directory.path().join("config.toml");
    config(&config_path, true);
    let output = directory.path().join("report");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("KnC Roll"));
    let detail: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-roll.json")).unwrap()).unwrap();
    assert_eq!(detail["test"], "roll");
    assert_eq!(detail["metrics"], serde_json::json!([]));
    // Left rises and right drops: corner mapping is not mirrored.
    assert_eq!(
        values(curve(&detail, "front_left", "wheel_center_z")),
        [0., 10., 20.]
    );
    assert_eq!(
        values(curve(&detail, "rear_right", "wheel_center_z")),
        [0., -10., -20.]
    );
    // Relative channels keep the subtracted absolute offset.
    let cop = curve(&detail, "front_right", "center_of_pressure_y");
    assert_eq!(values(cop), [0., 1., 2.]);
    assert_eq!(cop["offset"], -750.);
    // Axle signals stay absolute.
    let roll = curve(&detail, "total", "roll_angle");
    assert_eq!(values(roll), [0., 1., 2.]);
    assert!(roll.get("offset").is_none());
    assert_eq!(
        values(curve(&detail, "front", "roll_moment")),
        [0., 500., 1000.]
    );
    curve(&detail, "front", "suspension_roll_angle");
    curve(&detail, "total", "roll_moment");
    assert!(
        !detail["curves"]
            .as_array()
            .unwrap()
            .iter()
            .any(|curve| curve["corner"] == "rear" && curve["signal"] == "roll_moment")
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"][0]["test"], "roll");
    assert_eq!(
        fs::read_to_string(output.join("metrics.csv")).unwrap(),
        "run,domain,test,metric,unit,value\n"
    );
    assert!(!output.join("plots").exists());
}

#[test]
fn roll_requires_body_roll_angle_mapping() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("roll.h5");
    closed_fixture(&input, roll_measurement);
    let config_path = directory.path().join("config.toml");
    config(&config_path, false);
    let result = clab(&[&config_path, &input]);
    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("roll measurement signal mapping missing: total_roll_angle"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn roll_run_subtracts_static_baseline_from_corner_signals_only() {
    let directory = tempdir().unwrap();
    let simulation = directory.path().join("roll-simulation.h5");
    let static_simulation = directory.path().join("static-simulation.h5");
    closed_fixture(&simulation, |path| roll_simulation(path, "roll"));
    closed_fixture(&static_simulation, |path| roll_simulation(path, "static"));
    let config_path = directory.path().join("config.toml");
    config(&config_path, true);
    let run = directory.path().join("run.toml");
    fs::write(
        &run,
        "[meta]\nname=\"roll-run\"\nloadcase=\"L\"\n\
         [input.knc.roll]\nfile=\"roll-simulation.h5\"\n\
         [input.knc.static]\nfile=\"static-simulation.h5\"\n",
    )
    .unwrap();
    let output = directory.path().join("report");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            run.to_str().unwrap(),
            "-r",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let detail: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-roll.json")).unwrap()).unwrap();
    // Initial sample dropped; static final sample (left 320 mm) subtracted.
    let wcz = curve(&detail, "front_left", "wheel_center_z");
    assert_eq!(values(wcz), [-10., 0.]);
    assert_eq!(wcz["offset"], 320.);
    // The static file has no roll angle; roll angle needs no baseline.
    assert_eq!(values(curve(&detail, "total", "roll_angle")), [1., 2.]);
    assert_eq!(
        detail["normalization"]["method"],
        "drop_initial_sample_and_subtract_final_sample"
    );
}

#[test]
fn rejects_roll_combined_with_vertical_in_one_run() {
    let directory = tempdir().unwrap();
    let simulation = directory.path().join("roll-simulation.h5");
    closed_fixture(&simulation, |path| roll_simulation(path, "roll"));
    let vertical = directory.path().join("vertical-simulation.h5");
    closed_fixture(&vertical, |path| roll_simulation(path, "vertical"));
    let config_path = directory.path().join("config.toml");
    config(&config_path, true);
    let mut text = fs::read_to_string(&config_path).unwrap();
    for (corner, _) in CORNERS {
        for (name, component) in [
            ("travel", "travel"),
            ("wheel_center_x", "x"),
            ("wheel_center_y", "y"),
            ("wheel_center_z", "z"),
        ] {
            text.push_str(&format!(
                "[mappings.simulation.knc.vertical.signals.{corner}_{name}]\nentity=\"{corner}\"\ncomponent=\"{component}\"\nunit=\"mm\"\n"
            ));
        }
    }
    text.push_str(
        "[mappings.measurement.knc.vertical.signals.front_left_wheel_center_x]\nentity=\"lfwcx\"\nunit=\"mm\"\n",
    );
    fs::write(&config_path, text).unwrap();
    let run = directory.path().join("run.toml");
    fs::write(
        &run,
        "[input.knc.roll]\nfile=\"roll-simulation.h5\"\n[input.knc.vertical]\nfile=\"vertical-simulation.h5\"\n",
    )
    .unwrap();
    let result = clab(&[&config_path, &run]);
    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("combines [input.knc.roll]"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
