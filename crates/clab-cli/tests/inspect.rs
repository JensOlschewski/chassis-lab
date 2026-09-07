use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use hdf5::File;
use hdf5::types::VarLenUnicode;
use tempfile::tempdir;

const MINIMAL_RES: &str = r#"<Result>
  <Entity type="request" name="ride_heights" entity="vehicle.ride_heights" objectId="42">
    <Component name="front_left" unitsValue="mm" id="2" />
    <Component name="front_right" unitsValue="mm" id="1" />
  </Entity>
  <Entity type="request" name="svc_output">
    <Component name="Wheelbase" unitsValue="mm" id="3" />
  </Entity>
  <Step type="input">
    360.0 361.0 2490.0
  </Step>
  <Step type="input">
    359.5 360.5 2490.0
  </Step>
</Result>
"#;

fn inspect(path: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("inspect")
        .arg(path)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn inspects_simulation_hdf5_and_filters_signals() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../clab-converter/examples/example-svc.h5");
    let output = inspect(&path, &["--match", "wheelbase"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("source file: converter/examples/example-svc.res"));
    assert!(stdout.contains("format version: 3"));
    assert!(stdout.contains("samples: 1\nsignals: 25\nentities: 4"));
    assert!(stdout.contains("ENTITY\tCOMPONENT\tUNIT\tADAMS ID"));
    assert!(stdout.contains("example.geometry\twheelbase\tmm\t5"));
    assert_eq!(stdout.lines().filter(|line| line.contains('\t')).count(), 2);
}

#[test]
fn preserves_res_inspection() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("minimal.res");
    std::fs::write(&path, MINIMAL_RES).unwrap();
    let output = inspect(&path, &["--signals"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("2 samples, 3 signals\n"));
    assert!(stdout.contains("2\tvehicle.ride_heights\tfront_left\tmm"));
}

#[test]
fn inspects_measurement_hdf5_and_lists_channels() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../clab-converter/examples/example-vertical.h5");
    let output = inspect(&path, &["--signals"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("source file: example-vertical.acd"));
    assert!(stdout.contains("format version: 1"));
    assert!(stdout.contains("samples: 6\nchannels: 17\ntest metadata: 5"));
    assert!(stdout.contains("encoding: utf-8-sig\ndialect: ascii3"));
    assert!(stdout.contains("ORIGINAL NAME\tUNIT\tOFFSET\tCOLUMN\tDESCRIPTION"));
    assert!(stdout.contains("lfwcx\tmm\t0\t2\tFront left X"));
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
    let file = File::create(&unsupported).unwrap();
    file.new_attr::<VarLenUnicode>()
        .create("source_kind")
        .unwrap()
        .write_scalar(&VarLenUnicode::from_str("other").unwrap())
        .unwrap();
    drop(file);
    let output = inspect(&unsupported, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unsupported HDF5 source_kind 'other'; expected 'measurement'")
    );
}

#[test]
fn rejects_unsupported_extension() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("input.txt");
    std::fs::write(&path, "data").unwrap();
    let output = inspect(&path, &[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("unsupported input extension; expected .res, .h5, or .hdf5")
    );
}
