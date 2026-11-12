use std::{fs, path::Path, process::Command, str::FromStr};

use hdf5::types::VarLenUnicode;
use hdf5::{File, Group, Location};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

const CORNERS: [(&str, &str); 4] = [
    ("front_left", "lf"),
    ("front_right", "rf"),
    ("rear_left", "lr"),
    ("rear_right", "rr"),
];

const PLOTS: [&str; 6] = [
    "camber_vs_force_x",
    "toe_vs_force_x",
    "castor_vs_force_x",
    "proj_cp_x_vs_force_x",
    "wc_x_vs_force_x",
    "wc_y_vs_force_x",
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

/// Wheel force in N: rest, -4 kN, +4 kN, -4 kN, +4 kN, 0 in 200-sample legs.
fn force() -> Vec<f64> {
    let peaks = [0., -4000., 4000., -4000., 4000., 0.];
    peaks
        .windows(2)
        .flat_map(|leg| (0..200).map(move |i| leg[0] + (leg[1] - leg[0]) * i as f64 / 200.))
        .collect()
}

/// Every response is linear in the force: wheel center X 2 mm/kN, toe
/// +0.1 deg/kN as measured (the right side is flipped by the convention),
/// castor -0.6 deg/kN. Each channel carries a static offset.
fn measurement(path: &Path, setup: &str) {
    measurement_with_toe(path, setup, 0.1);
}

fn measurement_with_toe(path: &Path, setup: &str, toe_gain: f64) {
    let file = File::create(path).unwrap();
    let force = force();
    scalar(&file, "schema_version", 2_u32);
    scalar(&file, "sample_count", force.len() as u64);
    string(&file, "source_kind", "measurement");
    string(&file, "domain", "knc");
    string(&file, "setup", setup);
    string(&file, "test", "longitudinal");
    string(&file, "source_file", "fixture");
    let axis = file.create_group("axis").unwrap();
    string(&axis, "kind", "time");
    string(&axis, "unit", "s");
    axis.new_dataset_builder()
        .with_data(&(0..force.len()).map(|i| i as f64).collect::<Vec<_>>())
        .create("values")
        .unwrap();
    let signals = file.create_group("signals").unwrap();
    let linear = |gain: f64, offset: f64| -> Vec<f64> {
        force.iter().map(|n| offset + gain * n / 1000.).collect()
    };
    for (_, prefix) in CORNERS {
        channel(&signals, &format!("{prefix}fx"), &force, "N", 0.);
        for (name, unit, gain, offset) in [
            ("wcx", "mm", 2., 1200.),
            ("wcy", "mm", 0.1, 750.),
            ("pcpx", "mm", 1., 1200.),
            ("steer", "deg", toe_gain, 0.3),
            ("cam", "deg", 0.05, -1.),
            ("cas", "deg", -0.6, 6.),
        ] {
            channel(
                &signals,
                &format!("{prefix}{name}"),
                &linear(gain, offset),
                unit,
                offset,
            );
        }
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

fn config(path: &Path, omit: Option<&str>) {
    let mut text = String::from("[meta]\nname=\"fixture\"\nloadcase=\"L\"\n[setup.knc.metric]\n");
    for (corner, _) in CORNERS {
        text.push_str(&format!(
            "[mappings.simulation.knc.longitudinal.signals.{corner}_force_x]\n\
             entity=\"{corner}\"\ncomponent=\"fx\"\nunit=\"newton\"\n"
        ));
    }
    for (corner, prefix) in CORNERS {
        for (name, channel, unit) in [
            ("force_x", "fx", "N"),
            ("wheel_center_x", "wcx", "mm"),
            ("wheel_center_y", "wcy", "mm"),
            ("projected_contact_patch_x", "pcpx", "mm"),
            ("toe", "steer", "deg"),
            ("camber", "cam", "deg"),
            ("spin", "cas", "deg"),
        ]
        .into_iter()
        .filter(|(name, ..)| Some(*name) != omit)
        {
            text.push_str(&format!(
                "[mappings.measurement.knc.longitudinal.signals.{corner}_{name}]\n\
                 entity=\"{prefix}{channel}\"\nunit=\"{unit}\"\n"
            ));
        }
    }
    fs::write(path, text).unwrap();
}

fn analyze(setup: &str, omit: Option<&str>, report: bool) -> (std::process::Output, TempDir) {
    let directory = tempdir().unwrap();
    let input = directory.path().join("longitudinal.h5");
    let staging = input.with_extension("fixture.h5");
    measurement(&staging, setup);
    fs::copy(&staging, &input).unwrap();
    fs::remove_file(staging).unwrap();
    let config_path = directory.path().join("config.toml");
    config(&config_path, omit);
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.arg("analyze").arg(&config_path).arg(&input);
    if report {
        command.arg("-r").arg(directory.path().join("report"));
    }
    (command.output().unwrap(), directory)
}

fn metric(stdout: &str, name: &str) -> Option<f64> {
    let prefix = format!("knc::longitudinal::{name} ... ");
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
}

#[test]
fn braking_reports_brake_steer_and_windup_with_right_side_flip() {
    let (result, _directory) = analyze("b1e0f1r1", None, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("KnC Longitudinal\nrunning"), "{stdout}");
    assert!(stdout.contains("running 8 metrics\n"), "{stdout}");
    assert!(
        stdout.contains(
            "knc::longitudinal::longitudinal_compliance_front_left_mm_per_kn ... not covered (needs b0 setup) mm/kN"
        ),
        "{stdout}"
    );
    let near = |name: &str, expected: f64| {
        let value = metric(&stdout, name).unwrap_or_else(|| panic!("missing {name}\n{stdout}"));
        assert!((value - expected).abs() < 1e-9, "{name} = {value}");
    };
    near("brake_steer_front_left_deg_per_kn", 0.1);
    near("brake_steer_rear_right_deg_per_kn", -0.1);
    near("windup_compliance_front_right_deg_per_kn", -0.6);
}

#[test]
fn strapped_acceleration_reports_longitudinal_compliance_only() {
    let (result, _directory) = analyze("b0e0f1r1", None, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("running 4 metrics\n"), "{stdout}");
    for (corner, _) in CORNERS {
        let value = metric(
            &stdout,
            &format!("longitudinal_compliance_{corner}_mm_per_kn"),
        )
        .unwrap_or_else(|| panic!("missing {corner}\n{stdout}"));
        assert!((value - 2.0).abs() < 1e-9, "{corner} = {value}");
    }
    assert!(
        stdout
            .contains("brake_steer_front_left_deg_per_kn ... not covered (needs b1 setup) deg/kN"),
        "{stdout}"
    );
    assert!(metric(&stdout, "brake_steer_front_left_deg_per_kn").is_none());
}

/// Run manifest listing one longitudinal file per setup, in the given order.
fn run(setups: &[&str]) -> (std::process::Output, TempDir) {
    let directory = tempdir().unwrap();
    let mut manifest = String::from("[meta]\nname=\"fixture\"\nloadcase=\"L\"\n");
    for (index, setup) in setups.iter().enumerate() {
        let input = directory.path().join(format!("longitudinal-{index}.h5"));
        let staging = input.with_extension("fixture.h5");
        measurement(&staging, setup);
        fs::copy(&staging, &input).unwrap();
        fs::remove_file(staging).unwrap();
        manifest.push_str(&format!(
            "[[input.knc.longitudinal]]\nfile=\"{}\"\n",
            input.display()
        ));
    }
    let config_path = directory.path().join("config.toml");
    config(&config_path, None);
    let run_path = directory.path().join("run.toml");
    fs::write(&run_path, manifest).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("analyze")
        .arg(&config_path)
        .arg(&run_path)
        .arg("-r")
        .arg(directory.path().join("report"))
        .output()
        .unwrap();
    (output, directory)
}

#[test]
fn b0_and_b1_files_in_one_run_cover_every_metric() {
    let (result, directory) = run(&["b0e0f1r1", "b1e0f1r1"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(!stdout.contains("not covered"), "{stdout}");
    assert_eq!(stdout.matches("KnC Longitudinal").count(), 1, "{stdout}");
    assert!(stdout.contains("running 12 metrics\n"), "{stdout}");
    assert!(
        metric(&stdout, "longitudinal_compliance_front_left_mm_per_kn").is_some(),
        "{stdout}"
    );
    assert!(metric(&stdout, "brake_steer_front_left_deg_per_kn").is_some());
    let output = directory.path().join("report");
    for (setup, code) in [("b0", "b0e0f1r1"), ("b1", "b1e0f1r1")] {
        assert!(
            output
                .join(format!("knc-longitudinal-{setup}.json"))
                .is_file()
        );
        for plot in PLOTS {
            let path = output.join(format!(
                "plots/knc/longitudinal/knc_longitudinal_{plot}_{code}.pdf"
            ));
            let braking_only = plot.starts_with("castor") || plot.starts_with("proj_cp_x");
            assert_eq!(
                path.is_file(),
                setup == "b1" || !braking_only,
                "{plot} {setup}"
            );
        }
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"].as_array().unwrap().len(), 2);
    assert_eq!(report["summary"]["not_covered_count"], 0);
    assert_eq!(report["summary"]["metric_count"], 12);
}

#[test]
fn two_files_with_the_same_brake_state_are_refused() {
    let (result, _directory) = run(&["b0e0f1r1", "b0e0f1r1"]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("two longitudinal files with setup B0") && stderr.contains("ambiguous"),
        "{stderr}"
    );
}

#[test]
fn writes_report_with_all_six_plots_and_conditions() {
    let (result, directory) = analyze("b1e0f1r1", None, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = directory.path().join("report");
    for plot in PLOTS {
        let path = output.join(format!(
            "plots/knc/longitudinal/knc_longitudinal_{plot}_b1e0f1r1.pdf"
        ));
        assert!(path.is_file(), "{plot}");
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["domains"][0]["test"], "longitudinal");
    assert_eq!(report["domains"][0]["conditions"]["setup"], "b1e0f1r1");
    let series: Value = serde_json::from_str(
        &fs::read_to_string(output.join("knc-longitudinal-b1.plots.json")).unwrap(),
    )
    .unwrap();
    let marked: Vec<&str> = series["plots"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|plot| !plot["metrics"].as_array().unwrap().is_empty())
        .map(|plot| plot["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        marked,
        [
            "knc_longitudinal_castor_vs_force_x",
            "knc_longitudinal_toe_vs_force_x"
        ]
    );
    let csv = fs::read_to_string(output.join("metrics.csv")).unwrap();
    assert!(csv.contains(",knc,longitudinal,brake_steer_front_left_deg_per_kn,deg/kN,"));
    assert!(csv.contains(",b1e0f1r1,,"));
}

#[test]
fn missing_required_force_mapping_names_the_canonical_signal() {
    let (result, _directory) = analyze("b1e0f1r1", Some("force_x"), false);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("front_left_force_x"), "{stderr}");
}

#[test]
fn unmapped_response_makes_its_metric_unavailable_not_zero() {
    let (result, _directory) = analyze("b1e0f1r1", Some("spin"), false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("windup_compliance_front_left_deg_per_kn ... missing"),
        "{stdout}"
    );
    assert!(metric(&stdout, "brake_steer_front_left_deg_per_kn").is_some());
}

fn converted(directory: &Path, name: &str, setup: &str, toe_gain: f64) -> std::path::PathBuf {
    let input = directory.join(name);
    let staging = input.with_extension("fixture.h5");
    measurement_with_toe(&staging, setup, toe_gain);
    fs::copy(&staging, &input).unwrap();
    fs::remove_file(staging).unwrap();
    input
}

fn compare(
    directory: &Path,
    inputs: &[&Path],
    tolerances: bool,
    report: bool,
) -> std::process::Output {
    let config_path = directory.join("config.toml");
    config(&config_path, None);
    let mut text = fs::read_to_string(&config_path).unwrap();
    text.push_str(
        "[validation.knc.longitudinal.tolerances.absolute]\n\
         brake_steer_front_left_deg_per_kn=0.05\n\
         windup_compliance_front_left_deg_per_kn=0.01\n",
    );
    fs::write(&config_path, text).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.arg("compare").arg(&config_path).args(inputs);
    if tolerances {
        command.arg("--tolerances");
    }
    if report {
        command.arg("-r").arg(directory.join("report"));
    }
    command.output().unwrap()
}

#[test]
fn compare_checks_longitudinal_tolerances_like_other_tests() {
    let directory = tempdir().unwrap();
    let baseline = converted(directory.path(), "base.h5", "b1e0f1r1", 0.1);
    let candidate = converted(directory.path(), "candidate.h5", "b1e0f1r1", 0.2);
    let result = compare(directory.path(), &[&baseline, &candidate], true, false);
    assert_eq!(result.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let line = |name: &str| {
        stdout
            .lines()
            .find(|line| line.contains(&format!("knc::longitudinal::{name} ")))
            .unwrap_or_else(|| panic!("missing {name}\n{stdout}"))
    };
    assert!(
        line("brake_steer_front_left_deg_per_kn").contains("FAILED"),
        "{stdout}"
    );
    assert!(
        line("windup_compliance_front_left_deg_per_kn").contains("ok"),
        "{stdout}"
    );
    assert!(stdout.contains("0 metric missing"), "{stdout}");
    assert!(!stdout.contains("longitudinal_compliance"), "{stdout}");
}

#[test]
fn compare_uses_the_analyze_metrics_and_overlays_only_matching_setups() {
    let directory = tempdir().unwrap();
    let b0 = converted(directory.path(), "b0.h5", "b0e0f1r1", 0.1);
    let b1 = converted(directory.path(), "b1.h5", "b1e0f1r1", 0.1);
    let manifest = directory.path().join("run.toml");
    fs::write(
        &manifest,
        format!(
            "[meta]\nname=\"fixture\"\nloadcase=\"L\"\n\
             [[input.knc.longitudinal]]\nfile=\"{}\"\n\
             [[input.knc.longitudinal]]\nfile=\"{}\"\n",
            b0.display(),
            b1.display()
        ),
    )
    .unwrap();
    let result = compare(directory.path(), &[&manifest, &b1], false, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(stdout.matches("KnC Longitudinal").count(), 1, "{stdout}");
    assert!(
        stdout.contains("brake_steer_front_left_deg_per_kn"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("longitudinal_compliance_front_left_mm_per_kn ..."),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("4 metrics not compared"), "{stderr}");
    let comparison = directory.path().join("report/plots/knc/longitudinal");
    let names: Vec<_> = fs::read_dir(&comparison)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(
        names.iter().any(|name| name.ends_with("_b1e0f1r1.pdf")),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.contains("b0e0f1r1")),
        "{names:?}"
    );
    assert!(
        directory
            .path()
            .join("report/knc-longitudinal-b1.plots.json")
            .is_file()
    );
}

#[test]
fn compare_warns_when_longitudinal_conditions_differ() {
    let directory = tempdir().unwrap();
    let baseline = converted(directory.path(), "base.h5", "b1e0f1r1", 0.1);
    let candidate = converted(directory.path(), "candidate.h5", "b1e1f1r1", 0.1);
    let result = compare(directory.path(), &[&baseline, &candidate], false, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("test conditions differ (engine)"),
        "{stderr}"
    );
    assert!(stderr.contains("knc::longitudinal"), "{stderr}");
}

#[test]
fn compare_report_carries_longitudinal_conditions_in_json_csv_and_footer() {
    let directory = tempdir().unwrap();
    let baseline = converted(directory.path(), "base.h5", "b1e0f1r1", 0.1);
    let candidate = converted(directory.path(), "candidate.h5", "b1e1f1r1", 0.1);
    let result = compare(directory.path(), &[&baseline, &candidate], false, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = directory.path().join("report");
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("report.json")).unwrap()).unwrap();
    assert_eq!(
        report["candidates"][0]["domains"][0]["conditions"]["setup"],
        "b1e1f1r1"
    );
    let mut csv = csv::Reader::from_path(output.join("metrics.csv")).unwrap();
    let columns = csv.headers().unwrap().clone();
    let mut rows = 0;
    for row in csv.records() {
        let row = row.unwrap();
        let cell = |name| &row[columns.iter().position(|column| column == name).unwrap()];
        assert_eq!(cell("baseline_setup"), "b1e0f1r1");
        assert_eq!(cell("candidate_setup"), "b1e1f1r1");
        rows += 1;
    }
    assert!(rows > 0);
    let plots: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(output.join("knc-longitudinal-b1.plots.json")).unwrap(),
    )
    .unwrap();
    for plot in plots["plots"].as_array().unwrap() {
        let footer = plot["footer"].as_str().unwrap();
        assert!(footer.contains("b1e0f1r1"), "{footer}");
        assert!(footer.contains("b1e1f1r1"), "{footer}");
        assert!(
            footer.contains("test conditions differ (engine)"),
            "{footer}"
        );
    }
}

#[test]
fn compare_debug_renders_each_series_and_brake_state_in_flat_test_folder() {
    let directory = tempdir().unwrap();
    let b0 = converted(directory.path(), "b0.h5", "b0e0f1r1", 0.1);
    let b1 = converted(directory.path(), "b1.h5", "b1e0f1r1", 0.1);
    let manifest = directory.path().join("run.toml");
    fs::write(
        &manifest,
        format!(
            "[[input.knc.longitudinal]]\nfile=\"{}\"\n\
             [[input.knc.longitudinal]]\nfile=\"{}\"\n",
            b0.display(),
            b1.display()
        ),
    )
    .unwrap();
    let config_path = directory.path().join("config.toml");
    config(&config_path, None);
    let output = directory.path().join("report");
    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("compare")
        .args([&config_path, &manifest, &manifest, &manifest])
        .arg("--debug")
        .arg("--report")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let plots = output.join("plots/knc/longitudinal");
    assert!(!plots.join("comparison").exists());
    let debug = plots.join("debug");
    assert_eq!(fs::read_dir(&debug).unwrap().count(), 9);
    for series in 0..3 {
        for (setup, names) in [
            ("b0e0f1r1", vec!["wc_x"]),
            ("b1e0f1r1", vec!["toe", "castor"]),
        ] {
            for name in names {
                assert!(
                    debug
                        .join(format!(
                            "series_{series}_knc_longitudinal_{name}_vs_force_x_{setup}.pdf"
                        ))
                        .is_file()
                );
            }
        }
    }
    let html = fs::read_to_string(output.join("index.html")).unwrap();
    assert!(html.contains("plots/knc/longitudinal/knc_longitudinal_toe_vs_force_x_b1e0f1r1.pdf"));
    assert!(!html.contains("/comparison/"));
}
