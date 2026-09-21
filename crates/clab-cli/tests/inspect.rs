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
    assert!(stdout.contains("format version: 1"));
    assert!(stdout.contains("samples: 6\nchannels: 17\ntest metadata: 5"));
    assert!(stdout.contains("encoding: utf-8-sig\ndialect: ascii3"));
    assert!(stdout.contains("CHANNEL"));
    assert!(stdout.contains("SOURCE COLUMN"));
    assert!(stdout.contains("lfwcx"));
    assert!(stdout.contains("Front left X"));

    let matched = inspect(&path, &["--match", "lfwcx"]);
    assert!(matched.status.success());
    let stdout = String::from_utf8(matched.stdout).unwrap();
    assert!(stdout.contains("raw: 1199.800000..1200.200000"));
    assert!(stdout.contains("offset: 0.000000"));
    assert!(stdout.contains("normalized: 1199.800000..1200.200000"));
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
            .contains("invalid HDF5 input")
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
            .contains("unsupported HDF5 source_kind 'other'; expected 'measurement'")
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
