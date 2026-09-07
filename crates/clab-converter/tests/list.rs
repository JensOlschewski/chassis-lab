use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;

#[test]
fn lists_every_value_without_writing_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("minimal.res");
    fs::write(&input, common::MINIMAL_RES).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_adams-res"))
        .arg(&input)
        .arg("--list")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 4);
    assert!(stdout.contains("2\tvehicle.ride_heights\tfront_left\tmm"));
    assert!(stdout.contains("3\tsvc_output\tWheelbase\tmm"));
    assert!(!input.with_extension("h5").exists());
}
