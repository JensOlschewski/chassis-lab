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

/// `analysis_type` Q=0.0 declares this data as KnC, contradicting `--domain svc`.
const RES_DECLARING_KNC: &str = r#"<Result>
  <Entity type="request" name="analysis_type" entity="model.analysis_type" objectId="1">
    <Component name="Q" unitsValue="none" id="1" />
  </Entity>
  <Entity type="request" name="svc_output">
    <Component name="Wheelbase" unitsValue="mm" id="2" />
  </Entity>
  <Step type="input">
    0.0 2490.0
  </Step>
  <Step type="input">
    0.0 2490.0
  </Step>
</Result>
"#;

fn convert(input: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command
        .args(["convert", "--yes"])
        .arg(input)
        .args(["-d", "svc"]);
    command
}

#[test]
fn converts_res_to_neutral_default_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared.h5");
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
fn converts_svc_res_to_neutral_default_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared_svc.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared_svc.h5");
    assert!(output.is_file());
    assert_eq!(
        File::open(&output)
            .unwrap()
            .attr("schema_version")
            .unwrap()
            .read_scalar::<u32>()
            .unwrap(),
        2
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
fn converts_acd_to_neutral_default_hdf5() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.acd");
    fs::write(&input, MINIMAL_ACD).unwrap();

    let result = convert(&input).output().unwrap();

    assert!(result.status.success());
    let output = directory.path().join("shared.h5");
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
    assert!(output_dir.join("shared.h5").is_file());
}

#[test]
fn trailing_slash_output_creates_missing_directory() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();
    let output = format!("{}/nested/converted/", directory.path().display());

    let result = convert(&input).arg("-o").arg(&output).output().unwrap();

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        directory
            .path()
            .join("nested/converted/shared.h5")
            .is_file()
    );
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
    assert!(!directory.path().join("input.h5").exists());
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

#[test]
fn requires_domain_and_validates_test() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();
    let run = |args: &[&str]| {
        let result = Command::new(env!("CARGO_BIN_EXE_clab"))
            .arg("convert")
            .arg(&input)
            .args(args)
            .output()
            .unwrap();
        (
            result.status.code(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        )
    };

    let (code, stderr) = run(&[]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("--domain"), "{stderr}");
    for (args, message) in [
        (&["-d", "driving"][..], "unknown domain 'driving'"),
        (&["-d", "knc"][..], "--test <TEST>"),
        (
            &["-d", "knc", "-t", "lateral"][..],
            "unknown test 'lateral'",
        ),
        (&["-d", "svc", "-t", "vertical"][..], "has no tests"),
    ] {
        let (code, stderr) = run(args);
        assert_eq!(code, Some(2), "{args:?}");
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn stores_domain_and_test_when_data_has_no_identity_to_disagree_with() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, MINIMAL_RES).unwrap();
    let output = directory.path().join("out.h5");

    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("convert")
        .arg(&input)
        .args(["-d", "knc", "-t", "vertical", "--yes", "-o"])
        .arg(&output)
        .output()
        .unwrap();

    assert!(result.status.success());
    let file = hdf5::File::open(&output).unwrap();
    let read = |name: &str| {
        file.attr(name)
            .unwrap()
            .read_scalar::<hdf5::types::VarLenUnicode>()
            .unwrap()
            .as_str()
            .to_owned()
    };
    assert_eq!(
        (read("domain"), read("test")),
        ("knc".into(), "vertical".into())
    );
    // MINIMAL_RES has no motion channels, so the test cannot be verified.
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("no motion channels"), "{stderr}");
}

fn knc_res(roll: f64, vertical: f64) -> String {
    format!(
        r#"<Result>
  <Entity type="request" name="motion_roll" entity="rig.motion_roll" objectId="1">
    <Component name="R1" unitsValue="rad" id="1" />
  </Entity>
  <Entity type="request" name="motion_vertical" entity="rig.motion_vertical" objectId="2">
    <Component name="Z" unitsValue="mm" id="2" />
  </Entity>
  <Entity type="request" name="wheel_travel" entity="rig.wheel_travel" objectId="3">
    <Component name="Z" unitsValue="mm" id="3" />
  </Entity>
  <Step type="input">
    0.0 0.0 0.0
  </Step>
  <Step type="input">
    {roll} {vertical} 0.0
  </Step>
</Result>
"#
    )
}

fn convert_knc(input: &Path, test: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_clab"));
    command
        .arg("convert")
        .arg(input)
        .args(["-d", "knc", "-t", test])
        .stdin(std::process::Stdio::null());
    command
}

#[test]
fn matching_detected_test_converts_silently() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.res");
    fs::write(&input, knc_res(0.0, 100.0)).unwrap();

    let result = convert_knc(&input, "vertical").output().unwrap();

    assert!(result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("warning"));
}

#[test]
fn mismatched_test_without_terminal_aborts_and_writes_nothing() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.res");
    fs::write(&input, knc_res(0.0, 100.0)).unwrap();

    let result = convert_knc(&input, "roll").output().unwrap();

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("motion data indicates 'vertical'"),
        "{stderr}"
    );
    assert!(stderr.contains("pass --yes"), "{stderr}");
    assert!(!directory.path().join("run.h5").exists());
}

#[test]
fn mismatched_test_with_yes_warns_and_converts() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.res");
    fs::write(&input, knc_res(0.0, 100.0)).unwrap();

    let result = convert_knc(&input, "roll").arg("--yes").output().unwrap();

    assert!(result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("motion data indicates 'vertical'"),
        "{stderr}"
    );
    assert!(directory.path().join("run.h5").is_file());
}

#[test]
fn combined_motion_warns_and_converts() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.res");
    fs::write(&input, knc_res(0.14, 100.0)).unwrap();

    let result = convert_knc(&input, "roll").arg("--yes").output().unwrap();

    assert!(result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("combined test file"), "{stderr}");
}

#[test]
fn missing_motion_channels_abort_without_terminal() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.res");
    fs::write(&input, MINIMAL_RES).unwrap();

    let result = convert_knc(&input, "roll").output().unwrap();

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("no motion channels"), "{stderr}");
}

#[test]
fn acd_testtype_mismatch_warns_and_prompts() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("run.acd");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/input/raw-measurement/example-measurement-knc-roll.acd"),
        &input,
    )
    .unwrap();

    let result = convert_knc(&input, "vertical").output().unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr
            .contains("testtype 'roll' and motion data indicate 'roll', but --test is 'vertical'"),
        "{stderr}"
    );

    let result = convert_knc(&input, "roll").output().unwrap();
    assert!(result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("warning"));
}

#[test]
fn rejects_declared_domain_the_data_contradicts_and_writes_nothing() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("shared.res");
    fs::write(&input, RES_DECLARING_KNC).unwrap();
    let output = directory.path().join("out.h5");

    let result = Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("convert")
        .arg(&input)
        .args(["-d", "svc", "-o"])
        .arg(&output)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("analysis_type indicates 'knc', but --domain is 'svc'"),
        "{stderr}"
    );
    assert!(!output.exists());
}
