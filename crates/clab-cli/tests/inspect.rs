use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use std::{fs, path::PathBuf};

use hdf5::File;
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
    let source = examples.join("example-measurement-knc-vertical.acd");
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
    let source = examples.join("example-measurement-knc-vertical.acd");
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
    let legacy = directory.path().join("legacy.h5");
    std::fs::copy(&path, &legacy).unwrap();
    let file = hdf5::File::open_rw(&legacy).unwrap();
    file.attr("schema_version")
        .unwrap()
        .write_scalar(&1_u32)
        .unwrap();
    file.delete_attr("domain").unwrap();
    file.delete_attr("test").unwrap();
    file.close().unwrap();
    let result = inspect(&legacy);
    assert!(result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("warning:") && stderr.contains("schema version 1"),
        "{stderr}"
    );

    // Version 2 requires the domain.
    let broken = directory.path().join("broken.h5");
    std::fs::copy(&path, &broken).unwrap();
    let file = hdf5::File::open_rw(&broken).unwrap();
    file.delete_attr("domain").unwrap();
    file.close().unwrap();
    let result = inspect(&broken);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success(), "{stderr}");
    assert!(
        stderr.contains("missing HDF5 attribute: domain"),
        "{stderr}"
    );
}
