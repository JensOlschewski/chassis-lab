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

/// Body roll angle `roll` [deg]: left wheels rise and right wheels drop by
/// 10 mm/deg; both centres of pressure move 1 mm/deg to the left. The front
/// roll moment grows by `front_moment` Nm/deg.
fn roll_measurement(path: &Path, roll: &[f64], front_moment: f64) {
    let file = File::create(path).unwrap();
    header(&file, "measurement", "roll", roll.len() as u64);
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "time");
    string(&axis, "unit", "s");
    axis.new_dataset_builder()
        .with_data(
            &(0..roll.len())
                .map(|index| index as f64)
                .collect::<Vec<_>>(),
        )
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    let scaled = |gain: f64, offset: f64| -> Vec<f64> {
        roll.iter().map(|angle| offset + gain * angle).collect()
    };
    for (corner, prefix) in CORNERS {
        let side = if corner.ends_with("left") { 1. } else { -1. };
        let lateral = -side * 750.;
        channel(&signals, &format!("{prefix}wcx"), &scaled(0., 0.), "mm", 0.);
        channel(
            &signals,
            &format!("{prefix}wcy"),
            &scaled(0., lateral),
            "mm",
            lateral,
        );
        for name in ["wcz", "pz"] {
            channel(
                &signals,
                &format!("{prefix}{name}"),
                &scaled(side * 10., 300.),
                "mm",
                300.,
            );
        }
        channel(
            &signals,
            &format!("{prefix}copy"),
            &scaled(1., lateral),
            "mm",
            lateral,
        );
        channel(
            &signals,
            &format!("{prefix}cpy"),
            &scaled(1., lateral),
            "mm",
            lateral,
        );
        let stiffness = if corner.starts_with("front") {
            front_moment
        } else {
            400.
        };
        channel(
            &signals,
            &format!("{prefix}fz"),
            &scaled(side * stiffness / 1.5, 4000.),
            "N",
            0.,
        );
    }
    for (name, unit, gain) in [("troll", "deg", 1.), ("fsuspensionroll", "deg", 0.9)] {
        channel(&signals, name, &scaled(gain, 0.), unit, 0.);
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
        let gain = if corner.starts_with("front") {
            300.
        } else {
            400.
        };
        add(
            corner,
            "load",
            "newton",
            if test == "static" {
                [6000., 7000., 8000.]
            } else {
                [4000., 4000. - side * gain, 4000. + side * gain]
            },
        );
        add(
            corner,
            "cp_y",
            "mm",
            if test == "static" {
                [side * 0.01; 3]
            } else {
                [-side * 750., -side * 750. + 1., -side * 750. + 2.]
            },
        );
    }
    if test == "roll" {
        add("body", "roll", "deg", [0., -1., 1.]);
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

/// 3.5 cycles of +-3 deg: measurement drops the partial first sweep.
fn roll_cycles() -> Vec<f64> {
    (0..=700)
        .map(|index| 3. * (index as f64 * 7. * std::f64::consts::PI / 700.).sin())
        .collect()
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
            ("wheel_pad_z", "pz"),
            ("center_of_pressure_y", "copy"),
            ("contact_patch_y", "cpy"),
        ] {
            text.push_str(&format!(
                "[mappings.measurement.knc.roll.signals.{corner}_{name}]\nentity=\"{prefix}{channel}\"\nunit=\"mm\"\n"
            ));
        }
        text.push_str(&format!(
            "[mappings.measurement.knc.roll.signals.{corner}_wheel_load]\nentity=\"{prefix}fz\"\nunit=\"N\"\n"
        ));
        for (name, component) in [
            ("travel", "travel"),
            ("wheel_center_x", "x"),
            ("wheel_center_y", "y"),
            ("wheel_center_z", "z"),
            ("contact_patch_y", "cp_y"),
        ] {
            text.push_str(&format!(
                "[mappings.simulation.knc.roll.signals.{corner}_{name}]\nentity=\"{corner}\"\ncomponent=\"{component}\"\nunit=\"mm\"\n"
            ));
        }
        text.push_str(&format!(
            "[mappings.simulation.knc.roll.signals.{corner}_wheel_load]\nentity=\"{corner}\"\ncomponent=\"load\"\nunit=\"newton\"\n"
        ));
    }
    if roll_angle {
        text.push_str(
            "[mappings.measurement.knc.roll.signals.total_roll_angle]\nentity=\"troll\"\nunit=\"deg\"\n\
             [mappings.simulation.knc.roll.signals.total_roll_angle]\nentity=\"body\"\ncomponent=\"roll\"\nunit=\"deg\"\n",
        );
    }
    for (name, entity, unit) in [("front_suspension_roll_angle", "fsuspensionroll", "deg")] {
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
    closed_fixture(&input, |path| roll_measurement(path, &[0., 1., 2.], 500.));
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
    assert_eq!(cop["offset"], 750.);
    // Axle signals stay absolute.
    let roll = curve(&detail, "total", "roll_angle");
    assert_eq!(values(roll), [0., 1., 2.]);
    assert!(roll.get("offset").is_none());
    curve(&detail, "front", "suspension_roll_angle");
    assert!(
        !detail["curves"]
            .as_array()
            .unwrap()
            .iter()
            .any(|curve| curve["signal"] == "roll_moment")
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"][0]["test"], "roll");
    assert!(!output.join("plots").exists());
}

#[test]
fn analyzes_roll_metrics_from_measurement_cycles() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("roll.h5");
    closed_fixture(&input, |path| roll_measurement(path, &roll_cycles(), 500.));
    let config_path = directory.path().join("config.toml");
    config(&config_path, true);
    let output = directory.path().join("report");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            "--verbose",
            config_path.to_str().unwrap(),
            input.to_str().unwrap(),
            "-r",
        ])
        .arg(&output)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{stderr}");
    assert!(stderr.contains(
        "knc::roll: config key 'gradient_points_deg' missing in [setup.knc.metric]; using default 1 deg"
    ));
    assert!(stderr.contains(
        "knc::roll: metric 'roll_steer_front_left_deg_per_mm' unavailable: missing canonical signal front_left_toe"
    ));
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let metric = |name: &str| {
        report["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["metric"] == name)
            .unwrap_or_else(|| panic!("missing metric {name}"))
            .clone()
    };
    let stiffness = metric("roll_stiffness_front_nm_per_deg");
    assert_eq!(stiffness["test"], "roll");
    assert_eq!(stiffness["interval_deg"], serde_json::json!([-1., 1.]));
    assert!((stiffness["value"].as_f64().unwrap() - 500.).abs() < 1e-6);
    let distribution = metric("roll_stiffness_distribution_front_pct")["value"]
        .as_f64()
        .unwrap();
    assert!((distribution - 100. * 500. / 900.).abs() < 1e-6);
    // Contact paths at atan(0.1) over a 1500 mm track: rc_z = 75 mm.
    let height = metric("roll_center_height_front_mm");
    assert!(
        (height["value"].as_f64().unwrap() - 75.).abs() < 0.5,
        "{height}"
    );
    assert_eq!(height["lateral_signal"], "center_of_pressure_y");
    assert!(
        (metric("roll_wheel_rate_without_arb_front_left_n_per_mm")["value"]
            .as_f64()
            .unwrap()
            - 500. / 15.)
            .abs()
            < 1e-6
    );
    assert!(
        fs::read_to_string(output.join("metrics.csv"))
            .unwrap()
            .contains("fixture,knc,roll,roll_stiffness_front_nm_per_deg,Nm/deg,")
    );
}

#[test]
fn roll_requires_body_roll_angle_mapping() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("roll.h5");
    closed_fixture(&input, |path| roll_measurement(path, &[0., 1., 2.], 500.));
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
    let text = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        format!("{text}\n[setup.knc.roll.metric.simulation]\nindex=1\n"),
    )
    .unwrap();
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
    assert_eq!(values(curve(&detail, "total", "roll_angle")), [-1., 1.]);
    // Moment geometry and forces come from the roll run, not the static file.
    for (name, expected) in [
        ("roll_stiffness_front_nm_per_deg", 450.),
        ("roll_stiffness_rear_nm_per_deg", 600.),
        ("roll_stiffness_total_nm_per_deg", 1050.),
    ] {
        let metric = detail["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|metric| metric["metric"] == name)
            .unwrap();
        assert!(
            (metric["value"].as_f64().unwrap() - expected).abs() < 1e-6,
            "{metric}"
        );
    }
    assert_eq!(
        detail["normalization"]["method"],
        "drop_initial_sample_and_subtract_final_sample"
    );
}

#[test]
fn analyzes_and_compares_roll_combined_with_vertical_in_one_run() {
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
    let output = directory.path().join("report");
    let result = clab(&[&config_path, &run, Path::new("-r"), &output]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(stdout.contains("KnC Vertical"), "{stdout}");
    assert!(stdout.contains("KnC Roll"), "{stdout}");
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    let tests = report["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|domain| (domain["domain"].clone(), domain["test"].clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        tests,
        [
            (Value::from("knc"), Value::from("vertical")),
            (Value::from("knc"), Value::from("roll"))
        ]
    );
    assert!(output.join("knc-vertical.json").is_file());
    assert!(output.join("knc-roll.json").is_file());

    let compared = directory.path().join("compare");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("compare")
        .args([&config_path, &run, &run])
        .arg("-r")
        .arg(&compared)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(stdout.contains("KnC Vertical"), "{stdout}");
    assert!(stdout.contains("KnC Roll"), "{stdout}");
    assert!(compared.join("knc-vertical.json").is_file());
    assert!(compared.join("knc-roll.json").is_file());
}

#[test]
fn compares_roll_metrics_between_measurements() {
    let directory = tempdir().unwrap();
    let baseline = directory.path().join("baseline.h5");
    let candidate = directory.path().join("candidate.h5");
    closed_fixture(&baseline, |path| {
        roll_measurement(path, &roll_cycles(), 500.)
    });
    closed_fixture(&candidate, |path| {
        roll_measurement(path, &roll_cycles(), 600.)
    });
    let config_path = directory.path().join("config.toml");
    config(&config_path, true);
    let output = directory.path().join("report");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("compare")
        .args([&config_path, &baseline, &candidate])
        .arg("-r")
        .arg(&output)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(stdout.contains("KnC Roll"), "{stdout}");
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-roll.json")).unwrap()).unwrap();
    assert_eq!(
        report["domains"],
        serde_json::json!([{"domain": "knc", "test": "roll"}])
    );
    let stiffness = report["candidates"][0]["metrics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|metric| metric["metric"] == "roll_stiffness_front_nm_per_deg")
        .unwrap();
    assert_eq!(stiffness["test"], "roll");
    assert!((stiffness["difference"].as_f64().unwrap() - 100.).abs() < 1e-6);
    assert!(!output.join("plots").exists());
}
