use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use std::{fs, path::PathBuf};

use hdf5::File;
use hdf5::plist::file_access::FileCloseDegree;
use hdf5::types::VarLenUnicode;
use tempfile::tempdir;

fn inspect(path: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("inspect")
        .arg(path)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn inspects_svc_json_and_filters_signals() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("simulation.json");
    fs::write(
        &path,
        r#"{"schema_version":1,"source_kind":"simulation","source_file":"synthetic.res","sample_count":1,"values_per_sample":1,"entities":[{"name":"example.geometry","components":[{"name":"wheelbase","adams_id":5,"units":"mm","values":[2800.0]}]}]}"#,
    )
    .unwrap();
    let output = inspect(&path, &["--match", "wheelbase"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("synthetic.res"));
    assert!(stdout.contains("format version: 1"));
    assert!(stdout.contains("samples: 1\nsignals: 1"));
    assert!(stdout.contains("ENTITY\tCOMPONENT\tUNIT\tADAMS ID"));
    assert!(stdout.contains("example.geometry\twheelbase\tmm\t5"));
    assert_eq!(stdout.lines().filter(|line| line.contains('\t')).count(), 2);
}

#[test]
fn inspects_measurement_hdf5_and_lists_channels() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/input");
    let source = examples.join("raw-measurement/example-measurement-knc-vertical.acd");
    assert!(source.is_file(), "missing {}", source.display());
    let directory = tempdir().unwrap();
    let path = directory.path().join("measurement.h5");
    let converted = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "convert",
            source.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "vertical",
            "-o",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    let output = inspect(&path, &["--list"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("example-measurement-knc-vertical.acd"));
    assert!(stdout.contains("schema version: 2"));
    assert!(
        stdout.contains("source kind: measurement\nsamples: 4096\nsignals: 49\ntest metadata: 5")
    );
    assert!(stdout.contains("SIGNAL\tCOMPONENT\tUNIT\tSOURCE"));
    assert!(stdout.contains("lfwcx"));

    let matched = inspect(&path, &["--match", "lfwcx"]);
    assert!(matched.status.success());
    let stdout = String::from_utf8(matched.stdout).unwrap();
    assert!(stdout.contains("lfwcx"));
}

#[test]
fn rejects_invalid_hdf5_and_unsupported_source_kind() {
    let directory = tempdir().unwrap();
    let invalid = directory.path().join("invalid.h5");
    std::fs::write(&invalid, "not HDF5").unwrap();
    let output = inspect(&invalid, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("failed to inspect HDF5")
    );

    let unsupported = directory.path().join("unsupported.hdf5");
    let staging = PathBuf::from(format!("{}.fixture", unsupported.display()));
    let file = File::create(&staging).unwrap();
    file.new_attr::<VarLenUnicode>()
        .create("source_kind")
        .unwrap()
        .write_scalar(&VarLenUnicode::from_str("other").unwrap())
        .unwrap();
    file.close().unwrap();
    fs::copy(&staging, &unsupported).unwrap();
    fs::remove_file(staging).unwrap();
    let output = inspect(&unsupported, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("missing HDF5 attribute: schema_version")
    );
}

#[test]
fn rejects_raw_and_unsupported_extensions() {
    let directory = tempdir().unwrap();
    for extension in ["res", "acd", "txt"] {
        let path = directory.path().join(format!("input.{extension}"));
        std::fs::write(&path, "data").unwrap();
        let output = inspect(&path, &[]);

        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("clab inspect accepts .json, .h5, or .hdf5")
        );
    }
}

#[test]
fn inspect_warns_for_schema_version_1_and_rejects_v2_without_domain() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/input");
    let source = examples.join("raw-measurement/example-measurement-knc-vertical.acd");
    let directory = tempdir().unwrap();
    let path = directory.path().join("measurement.h5");
    let converted = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "convert",
            source.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "vertical",
            "-o",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(converted.status.success());
    let inspect = |path: &Path| {
        Command::new(env!("CARGO_BIN_EXE_clab"))
            .args(["inspect", path.to_str().unwrap()])
            .output()
            .unwrap()
    };

    let current = inspect(&path);
    assert!(current.status.success());
    assert!(String::from_utf8_lossy(&current.stdout).contains("schema version: 2"));
    assert!(!String::from_utf8_lossy(&current.stderr).contains("warning"));

    // Version 1: no domain or test, still readable, with a warning.
    let legacy_staging = directory.path().join("legacy-staging.h5");
    let legacy = directory.path().join("legacy.h5");
    fs::copy(&path, &legacy_staging).unwrap();
    // Edit a staging file, then copy it to a new inode. HDF5 may retain a
    // lock on the edited file even after its handles have been closed.
    let file = File::with_options()
        .with_fapl(|fapl| fapl.fclose_degree(FileCloseDegree::Strong))
        .open_rw(&legacy_staging)
        .unwrap();
    file.attr("schema_version")
        .unwrap()
        .write_scalar(&1_u32)
        .unwrap();
    file.delete_attr("domain").unwrap();
    file.delete_attr("test").unwrap();
    file.close().unwrap();
    fs::copy(&legacy_staging, &legacy).unwrap();
    let result = inspect(&legacy);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{stderr}");
    assert!(
        stderr.contains("warning:") && stderr.contains("schema version 1"),
        "{stderr}"
    );

    // Version 2 requires the domain.
    let broken_staging = directory.path().join("broken-staging.h5");
    let broken = directory.path().join("broken.h5");
    fs::copy(&path, &broken_staging).unwrap();
    let file = File::with_options()
        .with_fapl(|fapl| fapl.fclose_degree(FileCloseDegree::Strong))
        .open_rw(&broken_staging)
        .unwrap();
    file.delete_attr("domain").unwrap();
    file.close().unwrap();
    fs::copy(&broken_staging, &broken).unwrap();
    let result = inspect(&broken);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "{stderr}");
    assert!(
        stderr.contains("missing HDF5 attribute: domain"),
        "{stderr}"
    );
}

fn convert_example_measurement(directory: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/input/raw-measurement/example-measurement-knc-vertical.acd");
    let path = directory.join("measurement.h5");
    let converted = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args([
            "convert",
            source.to_str().unwrap(),
            "-d",
            "knc",
            "-t",
            "vertical",
        ])
        .args(["-o", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    path
}

fn inspect_json(path: &Path, arguments: &[&str]) -> serde_json::Value {
    let output = inspect(path, &[&["--json"], arguments].concat());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn json_lists_signals_and_reads_values_with_axis() {
    let directory = tempdir().unwrap();
    let path = convert_example_measurement(directory.path());

    let catalog = inspect_json(&path, &["--list"]);
    let signals = catalog["signals"].as_array().unwrap();
    assert_eq!(signals.len(), 49);
    let name = signals[0]["name"].as_str().unwrap().to_owned();
    assert!(signals[0]["unit"].is_string());

    let data = inspect_json(&path, &["--signal", &name]);
    assert_eq!(data["axis"].as_array().unwrap().len(), 4096);
    assert_eq!(data["signals"][0]["name"], name);
    assert_eq!(data["signals"][0]["values"].as_array().unwrap().len(), 4096);
}

#[test]
fn json_rejects_unknown_signal_bad_flags_and_non_hdf5() {
    let directory = tempdir().unwrap();
    let path = convert_example_measurement(directory.path());

    for arguments in [
        &["--json", "--signal", "no-such-signal"][..],
        &["--json"],
        &["--json", "--list", "--signal", "x"],
        &["--signal", "x"],
    ] {
        let output = inspect(&path, arguments);
        assert!(!output.status.success(), "{arguments:?}");
        assert!(output.stdout.is_empty(), "{arguments:?}");
    }

    let json = directory.path().join("simulation.json");
    fs::write(&json, "{}").unwrap();
    let output = inspect(&json, &["--json", "--list"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(".h5 or .hdf5"));
}
