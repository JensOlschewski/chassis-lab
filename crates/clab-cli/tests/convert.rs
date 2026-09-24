use std::fs;
use std::path::Path;
use std::process::Command;

use hdf5::File;
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

const MINIMAL_ACD: &str = "FileInfo: ASCII3\nTime\tWheel\tstring\tDescription\n0\t10\tfloat\tOffset\ntime\ttravel\tstring\tName\ns\tmm\tstring\tUnits\nNaN\tNaN\tstring\tKPoly\n0\t1\n";

fn convert(input: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command.arg("convert").arg(input);
    command
}

#[test]
fn converts_res_to_default_simulation_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared.simulation.h5");
    assert!(output.is_file());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!(
            "generated {} (canonical simulation HDF5)\n",
            output.display()
        )
    );
}

#[test]
fn converts_svc_res_to_default_simulation_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared_svc.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared_svc.simulation.h5");
    assert!(output.is_file());
    assert_eq!(
        File::open(&output)
            .unwrap()
            .attr("schema_version")
            .unwrap()
            .read_scalar::<u32>()
            .unwrap(),
        1
    );
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!(
            "generated {} (canonical simulation HDF5)\n",
            output.display()
        )
    );
}

#[test]
fn converts_acd_to_default_measurement_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.acd");
    fs::write(&input, MINIMAL_ACD).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared.measurement.h5");
    assert!(output.is_file());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!(
            "generated {} (canonical measurement HDF5)\n",
            output.display()
        )
    );
}

#[test]
fn existing_output_directory_uses_derived_filename() {
    let directory = tempdir().unwrap();
    let output_dir = directory.path().join("output");
    fs::create_dir(&output_dir).unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).arg("-o").arg(&output_dir).output().unwrap();

    assert!(result.status.success());
    assert!(output_dir.join("shared.simulation.h5").is_file());
}

#[test]
fn uses_explicit_output() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.res");
    let output = directory.path().join("chosen.hdf5");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).arg("-o").arg(&output).output().unwrap();

    assert!(result.status.success());
    assert!(output.is_file());
    assert!(!directory.path().join("input.simulation.h5").exists());
}

#[test]
fn requires_force_to_replace_output() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.res");
    let output = directory.path().join("output.h5");
    fs::write(&input, MINIMAL_RES).unwrap();
    fs::write(&output, "preserve me").unwrap();

    let rejected = convert(&input).arg("-o").arg(&output).output().unwrap();

    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8(rejected.stderr)
            .unwrap()
            .contains("already exists")
    );
    assert_eq!(fs::read_to_string(&output).unwrap(), "preserve me");

    let replaced = convert(&input)
        .arg("-o")
        .arg(&output)
        .arg("--force")
        .output()
        .unwrap();
    assert!(replaced.status.success());
    assert_ne!(fs::read(&output).unwrap(), b"preserve me");
}

#[test]
fn rejects_unsupported_extension() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.htm");
    fs::write(&input, "data").unwrap();

    let result = convert(&input).output().unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("unsupported input extension '.htm'")
    );
}

#[test]
fn rejects_non_hdf5_explicit_output() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    for name in ["output.json", "output.data"] {
        let result = convert(&input)
            .arg("-o")
            .arg(directory.path().join(name))
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("must use .h5 or .hdf5")
        );
    }
}

#[test]
fn rejects_missing_input_and_output_parent() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("missing.res");
    let missing_input = convert(&missing).output().unwrap();
    assert_eq!(missing_input.status.code(), Some(2));
    assert!(
        String::from_utf8(missing_input.stderr)
            .unwrap()
            .contains("input file does not exist")
    );

    let input = directory.path().join("input.res");
    fs::write(&input, MINIMAL_RES).unwrap();
    let missing_parent = convert(&input)
        .arg("-o")
        .arg(directory.path().join("missing/output.h5"))
        .output()
        .unwrap();
    assert_eq!(missing_parent.status.code(), Some(2));
    assert!(
        String::from_utf8(missing_parent.stderr)
            .unwrap()
            .contains("output parent directory does not exist")
    );
}

#[test]
fn rejects_input_as_output_even_with_force() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("input.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input)
        .arg("-o")
        .arg(&input)
        .arg("--force")
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("must not be the same as input")
    );
    assert_eq!(fs::read_to_string(input).unwrap(), MINIMAL_RES);
}

#[cfg(unix)]
#[test]
fn rejects_non_utf8_extension() {
    use std::os::unix::ffi::OsStringExt;

    let directory = tempdir().unwrap();
    let input = directory
        .path()
        .join(std::ffi::OsString::from_vec(b"input.\xff".to_vec()));
    fs::write(&input, "data").unwrap();

    let result = convert(&input).output().unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("extension is not valid UTF-8")
    );
}
