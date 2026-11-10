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
    simulation_with_test(path, "vertical");
}
fn static_simulation(path: &std::path::Path) {
    simulation_with_test(path, "static");
}
fn simulation_with_test(path: &std::path::Path, test: &str) {
    let file = File::create(path).unwrap();
    scalar(&file, "schema_version", 2_u32);
    scalar(&file, "sample_count", 5_u64);
    scalar(&file, "signal_count", 24_u64);
    string(&file, "source_kind", "simulation");
    string(&file, "domain", "knc");
    string(&file, "setup", "b1e0f1r1");
    string(&file, "test", test);
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
            ("cpx", [50., 48., 50., 52., 52.], 5),
            ("cpy", [60., 58., 60., 62., 62.], 6),
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
    } else if name.ends_with("steer") || name.ends_with("cam") || name.ends_with("cas") {
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
    measurement_with_front_left_wcx_name(path, "lfwcx");
}

/// Same fixture as `measurement`, but the front-left wheel-center-X channel
/// has a non-default name: proves `[measurement.knc.vertical.signals]`
/// mapping overrides, not just the conventional `lf`/`rf`/`lr`/`rr` names,
/// resolve a measurement channel.
fn measurement_renamed_channel(path: &std::path::Path) {
    measurement_with_front_left_wcx_name(path, "custom_lfwcx");
}

fn measurement_with_front_left_wcx_name(path: &std::path::Path, front_left_wcx_name: &str) {
    measurement_channels(path, front_left_wcx_name, Some("b1e0f1r1"));
}

fn measurement_channels(path: &std::path::Path, front_left_wcx_name: &str, setup: Option<&str>) {
    let file = File::create(path).unwrap();
    scalar(&file, "schema_version", 2_u32);
    scalar(&file, "sample_count", 21_u64);
    scalar(&file, "signal_count", 48_u64);
    string(&file, "source_kind", "measurement");
    string(&file, "domain", "knc");
    if let Some(setup) = setup {
        string(&file, "setup", setup);
    }
    string(&file, "test", "vertical");
    string(&file, "source_file", "fixture.acd");
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "time");
    string(&axis, "unit", "s");
    axis.new_dataset_builder()
        .with_data(&[0.; 21])
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    for prefix in ["lf", "rf", "lr", "rr"] {
        channel(
            &signals,
            &format!("{prefix}slope"),
            &[
                2., 2., 2., 2., 2., 2., 2., 2., 2., 2., 3., 3., 3., 3., 3., 3., 3., 3., 3., 3., 3.,
            ],
            0.,
        );
        let wcx_name = if prefix == "lf" {
            front_left_wcx_name.to_string()
        } else {
            format!("{prefix}wcx")
        };
        channel(
            &signals,
            &wcx_name,
            &[
                100., 100.5, 101., 101.5, 102., 101.5, 101., 100.5, 100., 99.5, 99., 98.5, 98.,
                98.5, 99., 99.5, 100., 100.5, 101., 101.5, 102.,
            ],
            100.,
        );
        channel(
            &signals,
            &format!("{prefix}wcy"),
            &[
                200., 200.75, 201.5, 202.25, 203., 202.25, 201.5, 200.75, 200., 199.25, 198.5,
                197.75, 197., 197.75, 198.5, 199.25, 200., 200.75, 201.5, 202.25, 203.,
            ],
            200.,
        );
        channel(
            &signals,
            &format!("{prefix}wcz"),
            &[
                300., 300.25, 300.5, 300.75, 301., 300.75, 300.5, 300.25, 300., 299.75, 299.5,
                299.25, 299., 299.25, 299.5, 299.75, 300., 300.25, 300.5, 300.75, 301.,
            ],
            300.,
        );
        channel(
            &signals,
            &format!("{prefix}steer"),
            &[
                0., 0.005, 0.01, 0.015, 0.02, 0.015, 0.01, 0.005, 0., -0.005, -0.01, -0.015, -0.02,
                -0.015, -0.01, -0.005, 0., 0.005, 0.01, 0.015, 0.02,
            ],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}cam"),
            &[
                0., -0.0075, -0.015, -0.0225, -0.03, -0.0225, -0.015, -0.0075, 0., 0.0075, 0.015,
                0.0225, 0.03, 0.0225, 0.015, 0.0075, 0., -0.0075, -0.015, -0.0225, -0.03,
            ],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}cas"),
            &[
                0., -0.01, -0.02, -0.03, -0.04, -0.03, -0.02, -0.01, 0., 0.01, 0.02, 0.03, 0.04,
                0.03, 0.02, 0.01, 0., -0.01, -0.02, -0.03, -0.04,
            ],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}fz"),
            &[
                3000., 3007.5, 3015., 3022.5, 3030., 3022.5, 3015., 3007.5, 3000., 2992.5, 2985.,
                2977.5, 2970., 2977.5, 2985., 2992.5, 3000., 3007.5, 3015., 3022.5, 3030.,
            ],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}pz"),
            &[
                0., 0.275, 0.55, 0.825, 1.1, 0.825, 0.55, 0.275, 0., -0.275, -0.55, -0.825, -1.1,
                -0.825, -0.55, -0.275, 0., 0.275, 0.55, 0.825, 1.1,
            ],
            0.,
        );
        channel(
            &signals,
            &format!("{prefix}tdef"),
            &[
                17., 17.025, 17.05, 17.075, 17.1, 17.075, 17.05, 17.025, 17., 16.975, 16.95,
                16.925, 16.9, 16.925, 16.95, 16.975, 17., 17.025, 17.05, 17.075, 17.1,
            ],
            17.,
        );
        channel(
            &signals,
            &format!("{prefix}cpx"),
            &[
                50., 50.5, 51., 51.5, 52., 51.5, 51., 50.5, 50., 49.5, 49., 48.5, 48., 48.5, 49.,
                49.5, 50., 50.5, 51., 51.5, 52.,
            ],
            50.,
        );
        channel(
            &signals,
            &format!("{prefix}cpy"),
            &[
                60., 60.5, 61., 61.5, 62., 61.5, 61., 60.5, 60., 59.5, 59., 58.5, 58., 58.5, 59.,
                59.5, 60., 60.5, 61., 61.5, 62.,
            ],
            60.,
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
            ("contact_patch_x", "cpx"),
            ("contact_patch_y", "cpy"),
        ] {
            if omit && corner == "front_left" && name == "wheel_center_x" {
                continue;
            }
            signals.push_str(&format!("[mappings.simulation.knc.vertical.signals.{corner}_{name}]\nentity = \"{corner}\"\ncomponent = \"{component}\"\nunit = \"mm\"\n"));
        }
    }
    let mut measurement_signals = String::new();
    for (corner, prefix) in [
        ("front_left", "lf"),
        ("front_right", "rf"),
        ("rear_left", "lr"),
        ("rear_right", "rr"),
    ] {
        for (name, channel, unit) in [
            ("wheel_center_x", "wcx", "mm"),
            ("wheel_center_y", "wcy", "mm"),
            ("wheel_center_z", "wcz", "mm"),
            ("wheel_load", "fz", "N"),
            ("toe", "steer", "deg"),
            ("camber", "cam", "deg"),
            ("spin", "cas", "deg"),
            ("wheel_pad_z", "pz", "mm"),
            ("tire_deflection", "tdef", "mm"),
            ("contact_patch_x", "cpx", "mm"),
            ("contact_patch_y", "cpy", "mm"),
        ] {
            measurement_signals.push_str(&format!(
                "[mappings.measurement.knc.vertical.signals.{corner}_{name}]\nentity = \"{prefix}{channel}\"\nunit = \"{unit}\"\n"
            ));
        }
    }
    fs::write(path,format!("[meta]\nname=\"fixture\"\nloadcase=\"L\"\n[physics]\ngravity=9.80665\n[setup.knc.vertical]\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n{signals}{measurement_signals}" )).unwrap();
}

/// Real reverse-engineered VARB corner masses; ride_frequency's fallback when
/// no SVC domain runs alongside KnC vertical in the same analyze/compare.
const SPRUNG_MASS_KG: &str = "sprung_mass_front_left_kg=449.0\nsprung_mass_front_right_kg=427.0\nsprung_mass_rear_left_kg=384.0\nsprung_mass_rear_right_kg=383.0\n";

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
    let svg = output.join("plots/knc/vertical/knc_vertical_wcz_vs_wcy_b1e0f1r1.pdf");
    let recession = output.join("plots/knc/vertical/knc_vertical_recession_angle_b1e0f1r1.pdf");
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
            .join("plots/knc/vertical/knc_vertical_wcz_vs_wcx_b1e0f1r1.pdf")
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

/// Measurement channel naming is mapping-driven, not hardcoded: a channel
/// under a non-conventional name resolves once
/// `[measurement.knc.vertical.signals]` is pointed at it, proving the
/// override reaches the measurement path the same way simulation
/// entity/component mappings already do.
#[test]
fn analyzes_measurement_vertical_with_renamed_channel_via_mapping() {
    let directory = tempdir().unwrap();
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&measurement_path, measurement_renamed_channel);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let text = fs::read_to_string(&config_path).unwrap().replace(
        "[mappings.measurement.knc.vertical.signals.front_left_wheel_center_x]\nentity = \"lfwcx\"\nunit = \"mm\"\n",
        "[mappings.measurement.knc.vertical.signals.front_left_wheel_center_x]\nentity = \"custom_lfwcx\"\nunit = \"mm\"\n",
    );
    fs::write(&config_path, text).unwrap();
    let output = directory.path().join("reports");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "analyze",
            config_path.to_str().unwrap(),
            measurement_path.to_str().unwrap(),
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
    let detail: Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-vertical.json")).unwrap())
            .unwrap();
    let front_left_wheel_center_x = detail["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|curve| curve["corner"] == "front_left" && curve["signal"] == "wheel_center_x")
        .expect("front_left wheel_center_x curve");
    assert_eq!(
        front_left_wheel_center_x["values"],
        serde_json::json!([
            0.0, 0.5, 1.0, 1.5, 2.0, 1.5, 1.0, 0.5, 0.0, -0.5, -1.0, -1.5, -2.0, -1.5, -1.0, -0.5,
            0.0, 0.5, 1.0, 1.5, 2.0
        ])
    );
}

#[test]
fn analyzes_measurement_vertical_gradient_metrics() {
    let directory = tempdir().unwrap();
    let measurement_path = directory.path().join("measurement.h5");
    closed_fixture(&measurement_path, measurement);
    // Explicit gradient points, and the reported default.
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
        format!(
            "{text}\n[setup.knc.vertical.metric]\n{gradient}direction='any'\nindex=1\n{SPRUNG_MASS_KG}"
        ),
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
    assert!(
        stdout.contains("KnC Vertical\nrunning") && stdout.contains("running 36 metrics"),
        "{stdout}"
    );
    assert_vertical_console_order(&stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    if gradient.is_empty() {
        // The 10 mm default exceeds the ±1 mm fixture, so metrics are unavailable.
        assert!(stderr.contains(
            "warning: knc::vertical: config key 'gradient_points_mm' missing in [setup.knc.vertical]; \
             using default 10 mm\n"
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
    close(value("bump_spin_front_left_deg_per_mm"), -0.04);
    close(value("bump_spin_rear_right_deg_per_mm"), -0.04);
    close(
        value("wheel_recession_angle_front_left_deg"),
        (-2.0_f64).atan().to_degrees(),
    );
    close(value("suspension_rate_rear_right_n_per_mm"), 30.0);
    close(value("ride_rate_rear_right_n_per_mm"), 30.0 / 1.1);
    close(value("tire_rate_rear_right_n_per_mm"), 300.0);
    let csv = fs::read_to_string(output.join("metrics.csv")).unwrap();
    assert!(csv.starts_with("run,domain,test,metric,unit,value,setup,phase,offset_mm\n"));
    let mut rows = csv::Reader::from_reader(csv.as_bytes());
    for row in rows.records() {
        let row = row.unwrap();
        assert_eq!(&row[6], "b1e0f1r1");
        assert_eq!(&row[7], "");
        assert_eq!(&row[8], "");
    }
    let rate = csv::Reader::from_reader(csv.as_bytes())
        .records()
        .map(Result::unwrap)
        .find(|row| &row[3] == "suspension_rate_rear_right_n_per_mm")
        .expect("rear right suspension rate row");
    assert_eq!(&rate[0], "fixture");
    assert_eq!(&rate[4], "N/mm");
    close(rate[5].parse().unwrap(), 30.0);
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"][0]["metric_count"], 36);
    assert_eq!(
        report["domains"][0]["conditions"],
        serde_json::json!({"setup": "b1e0f1r1"})
    );
}

#[test]
fn committed_examples_convert_and_produce_vertical_metrics() {
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let directory = tempdir().unwrap();
    let convert = |source: &str, output: &str, test: &str| {
        let destination = directory.path().join(output);
        let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
        command
            .args(["convert", "--yes"])
            .arg(examples.join("input").join(source))
            .args(["-d", "knc", "-t", test, "--setup", "b1e0f1r1", "-o"])
            .arg(&destination);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        destination
    };
    let measurement = convert(
        "raw-measurement/example-measurement-knc-vertical.acd",
        "measurement.h5",
        "vertical",
    );
    let simulation = convert(
        "raw-simulation/example-simulation-knc-vertical.res",
        "simulation.h5",
        "vertical",
    );
    let static_input = convert(
        "raw-simulation/example-simulation-knc-static.res",
        "static.h5",
        "static",
    );
    for (path, samples) in [(&simulation, 182), (&static_input, 106)] {
        let file = clab_core::canonical_hdf5::CanonicalFile::open(path).unwrap();
        assert_eq!(file.read_axis().unwrap().len(), samples);
        assert_eq!(file.signals().len(), 44);
    }
    let run = directory.path().join("simulation.toml");
    fs::write(
        &run,
        format!(
            "[input.knc.static]\nfile = {:?}\n[input.knc.vertical]\nfile = {:?}\n",
            static_input, simulation
        ),
    )
    .unwrap();

    for input in [&measurement, &run] {
        let result = Command::new(env!("CARGO_BIN_EXE_clab"))
            .arg("analyze")
            .arg(examples.join("config.toml"))
            .arg(input)
            .arg("--verbose")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(
            stdout.contains("KnC Vertical\nrunning") && stdout.contains("running 36 metrics"),
            "{stdout}"
        );
        // Neither input has SVC/sprung-mass data, so ride_frequency (4 rows)
        // is always unavailable. Both fixtures now carry real-derived
        // contact-patch data, resolved through examples/mappings.toml either
        // way (the simulation `run` via `[simulation.knc.vertical.signals]`
        // entity mappings, the measurement fixture via
        // `[measurement.knc.vertical.signals]` channel mappings), so
        // brake_support_angle computes for both. Every other metric must
        // compute for both.
        let expected_missing = 4;
        assert_eq!(
            stdout.matches("... missing").count(),
            expected_missing,
            "{stdout}"
        );
        assert!(
            !stdout.lines().any(|line| line.contains("... missing")
                && !line.contains("brake_support_angle")
                && !line.contains("ride_frequency")),
            "{stdout}"
        );
        let rate = stdout
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("knc::vertical::suspension_rate_front_left_n_per_mm ... ")
            })
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let expected = if input == &measurement {
            29.59007013963512
        } else {
            29.133052308269015
        };
        assert!((rate - expected).abs() < 1e-6, "{stdout}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        // The real ADAMS toe/camber request omits units: use its declared degree mapping.
        let warnings: Vec<_> = stderr
            .lines()
            .filter(|line| line.starts_with("warning:"))
            .collect();
        // Same split as the stdout check above, on top of whatever
        // legacy-unit warnings this input has.
        let (unavailable, other): (Vec<&&str>, Vec<&&str>) = warnings
            .iter()
            .partition(|line| line.contains(" unavailable: "));
        assert_eq!(unavailable.len(), expected_missing, "{stderr}");
        assert_eq!(
            other.len(),
            if input == &measurement { 0 } else { 8 },
            "{stderr}"
        );
        assert!(
            other.iter().all(|line| line.contains("toe_camber_body/")
                && line.contains("using mapped unit 'deg'")),
            "{stderr}"
        );
    }
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
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("KnC Vertical\nrunning") && stdout.contains("running 0 metrics"));
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("knc::vertical: metrics require static_simulation normalization")
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
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        !stdout.contains("Setup  ") && !stdout.contains("Input  "),
        "{stdout}"
    );
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("analyze")
        .args([&config_path, &simulation_path])
        .args(["--json", "--label", "VARB"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let section: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        section["section"]["conditions"],
        serde_json::json!({"setup": "b1e0f1r1"})
    );
    assert_eq!(section["section"]["name"], "VARB");
    assert_eq!(
        section["section"]["input"],
        simulation_path.to_str().unwrap()
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
    let verbose_stderr = String::from_utf8_lossy(&verbose.stderr);
    assert!(verbose_stderr.contains("no HDF5 unit metadata"));
    assert!(verbose_stderr.contains("warning: knc::vertical: simulation signal"));
}

#[test]
fn run_simulation_static_normalizes_and_reports_available_metrics() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    let static_path = directory.path().join("static.h5");
    closed_fixture(&simulation_path, simulation);
    closed_fixture(&static_path, static_simulation);
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

#[test]
fn analyze_debug_warns_and_skips_rate_plots_for_simulation_input() {
    let directory = tempdir().unwrap();
    let simulation_path = directory.path().join("simulation.h5");
    closed_fixture(&simulation_path, simulation);
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
            "--debug",
            "--verbose",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains(
        "warning: knc::vertical: debug rate plots require measurement input; skipped for simulation data"
    ));
}

/// Measurement config with the first sweep selected, so every metric exists.
fn comparison_config(path: &std::path::Path, tolerance: &str) {
    config(path, false);
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(
            format!(
                "\n[setup.knc.vertical.metric]\ngradient_points_mm=0.5\n{SPRUNG_MASS_KG}\n\
                 [setup.knc.vertical.branch]\ndirection='any'\nindex=1\n{tolerance}"
            )
            .as_bytes(),
        )
        .unwrap();
}

const EXPECTED_METRICS: [(&str, &str); 9] = [
    ("bump_camber", "deg_per_mm"),
    ("bump_steer", "deg_per_mm"),
    ("bump_spin", "deg_per_mm"),
    ("brake_support_angle", "deg"),
    ("wheel_recession_angle", "deg"),
    ("ride_frequency", "hz"),
    ("ride_rate", "n_per_mm"),
    ("suspension_rate", "n_per_mm"),
    ("tire_rate", "n_per_mm"),
];

fn assert_vertical_console_order(stdout: &str) {
    let expected: Vec<_> = EXPECTED_METRICS
        .iter()
        .flat_map(|(family, unit)| {
            ["front_left", "front_right", "rear_left", "rear_right"]
                .map(move |corner| format!("{family}_{corner}_{unit}"))
        })
        .collect();
    let actual: Vec<_> = stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("knc::vertical::"))
        .map(|line| line.split(" ...").next().unwrap())
        .collect();
    assert_eq!(actual, expected, "{stdout}");
}

#[test]
fn compares_only_gradient_metrics_with_overlays() {
    let directory = tempdir().unwrap();
    let baseline_path = directory.path().join("baseline.h5");
    let candidate_path = directory.path().join("candidate.h5");
    closed_fixture(&baseline_path, measurement);
    closed_fixture(&candidate_path, |path| {
        measurement_channels(path, "lfwcx", Some("b1e1f1r1"))
    });
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
    assert_eq!(report["domains"][0]["conditions"]["setup"], "b1e0f1r1");
    assert_eq!(
        report["candidates"][0]["domains"][0]["conditions"]["setup"],
        "b1e1f1r1"
    );
    assert!(
        report["warnings"][0]
            .as_str()
            .unwrap()
            .contains("test conditions differ (engine)")
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("test conditions differ (engine)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Baseline b1e0f1r1 vs Curve 1 b1e1f1r1"),
        "{stderr}"
    );
    let mut expected = Vec::new();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        for (metric, unit) in EXPECTED_METRICS {
            expected.push(format!("{metric}_{corner}_{unit}"));
        }
    }
    expected.sort();
    let metrics = report["candidates"][0]["metrics"].as_array().unwrap();
    let mut listed = metrics
        .iter()
        .map(|metric| metric["metric"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    listed.sort();
    assert_eq!(listed, expected);
    assert!(
        metrics
            .iter()
            .all(|metric| metric["status"] == "informational")
    );
    assert_eq!(report["status"], "informational");
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("KnC Vertical\nbaseline:"));
    assert_vertical_console_order(&stdout);
    assert!(stdout.contains("knc::vertical::bump_steer_front_left_deg_per_mm ... (baseline"));
    assert!(!stdout.contains("wheel_center_x_mm"));
    let svg = output.join("plots/knc/vertical/comparison/knc_vertical_wcz_vs_wcy.pdf");
    assert!(fs::read(svg).unwrap().starts_with(b"%PDF"));
    // comparison_config sets sprung_mass_*_kg, so ride_frequency's overlay
    // plot renders for both baseline and candidate series.
    let ride_frequency =
        output.join("plots/knc/vertical/comparison/knc_vertical_ride_frequency.pdf");
    assert!(fs::read(ride_frequency).unwrap().starts_with(b"%PDF"));
    assert!(output.join("metrics.csv").is_file());
    let mut csv = csv::Reader::from_path(output.join("metrics.csv")).unwrap();
    let columns = csv.headers().unwrap().clone();
    for row in csv.records() {
        let row = row.unwrap();
        let cell = |name| &row[columns.iter().position(|column| column == name).unwrap()];
        assert_eq!(cell("baseline_setup"), "b1e0f1r1");
        assert_eq!(cell("candidate_setup"), "b1e1f1r1");
    }

    // The app draws the same overlays from data: baseline and candidate are one source each,
    // and the metrics a plot highlights are all listed in report.json.
    let plots: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("knc-vertical.plots.json")).unwrap())
            .unwrap();
    assert_eq!(plots["command"], "compare");
    for plot in plots["plots"].as_array().unwrap() {
        let footer = plot["footer"].as_str().unwrap();
        assert!(footer.contains("b1e0f1r1"), "{footer}");
        assert!(footer.contains("b1e1f1r1"), "{footer}");
        assert!(
            footer.contains("WARNING: knc::vertical: test conditions differ (engine)"),
            "{footer}"
        );
        for panel in plot["panels"].as_array().unwrap() {
            assert_eq!(panel["sources"].as_array().unwrap().len(), 2);
        }
        for name in plot["metrics"].as_array().unwrap() {
            assert!(
                listed.contains(&name.as_str().unwrap().to_owned()),
                "{name}"
            );
        }
    }
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
    assert!(!String::from_utf8_lossy(&result.stderr).contains("test conditions differ"));
}

#[test]
fn condition_differences_are_in_compare_json_without_opt_in() {
    let directory = tempdir().unwrap();
    let baseline = directory.path().join("baseline.h5");
    let candidate = directory.path().join("candidate.h5");
    closed_fixture(&baseline, measurement);
    closed_fixture(&candidate, |path| {
        measurement_channels(path, "lfwcx", Some("b0e1f0r0"))
    });
    let config_path = directory.path().join("config.toml");
    comparison_config(&config_path, "");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("compare")
        .args([&config_path, &baseline, &candidate])
        .args(["--json", "--label", "VARB", "--label", "VARC"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    let section: Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    let section = &section["section"];
    assert_eq!(section["inputs"][0]["label"], "VARB");
    assert_eq!(section["inputs"][1]["label"], "VARC");
    assert_eq!(
        section["inputs"][1]["conditions"]["vertical"]["setup"],
        "b0e1f0r0"
    );
    let warning = section["warnings"][0].as_str().unwrap();
    assert!(
        warning.contains("brakes, engine, front ARB, rear ARB"),
        "{warning}"
    );
    assert!(
        warning.contains("VARB b1e0f1r1 vs VARC b0e1f0r0"),
        "{warning}"
    );
}

#[test]
fn compare_rejects_missing_or_invalid_setup() {
    let directory = tempdir().unwrap();
    let baseline = directory.path().join("baseline.h5");
    closed_fixture(&baseline, measurement);
    let config_path = directory.path().join("config.toml");
    comparison_config(&config_path, "");
    for setup in [None, Some("unknown")] {
        let candidate = directory.path().join("candidate.h5");
        // Finish writing and close the HDF5 before the CLI reads it.
        let staging = directory.path().join("staging.h5");
        measurement_channels(&staging, "lfwcx", setup);
        fs::copy(&staging, &candidate).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_clab"))
            .arg("compare")
            .args([&config_path, &baseline, &candidate])
            .output()
            .unwrap();
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        let expected = if setup.is_none() {
            "convert it again with `clab convert --setup`"
        } else {
            "invalid setup 'unknown'"
        };
        assert!(stderr.contains(expected), "{stderr}");
        assert!(stderr.contains("candidate.h5"), "{stderr}");
    }
}

#[test]
fn compare_debug_warns_and_skips_rate_plots_for_simulation_input() {
    let directory = tempdir().unwrap();
    let baseline_path = directory.path().join("baseline.h5");
    let candidate_path = directory.path().join("candidate.h5");
    closed_fixture(&baseline_path, simulation);
    closed_fixture(&candidate_path, simulation);
    let config_path = directory.path().join("case.toml");
    config(&config_path, false);
    let output = directory.path().join("comparison");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "compare",
            config_path.to_str().unwrap(),
            baseline_path.to_str().unwrap(),
            candidate_path.to_str().unwrap(),
            "-r",
            output.to_str().unwrap(),
            "--debug",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains(
        "warning: knc::vertical: debug rate plots require measurement input; skipped for simulation data"
    ));
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
