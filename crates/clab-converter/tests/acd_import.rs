use std::fs;
use std::path::PathBuf;

use clab_converter::measurement_hdf5;
use clab_core::measurement_hdf5::MeasurementFile;
use tempfile::tempdir;

fn write_acd(path: &std::path::Path, names: &str) {
    fs::write(
        path,
        format!(
            "\u{feff}FileInfo: ASCII3\nTestInfo:\tsetup\tsetup\tTest setup\tstring\tNaN\twet\nTime\tWheel travel\tDuplicate\tstring\tDescription\n0\t10\t0\tfloat\tOffset\n{names}\tstring\tName\ns\tmm\tN\tstring\tUnits\nNaN\tNaN\tNaN\tstring\tKPoly\n0\tNaN\t-Inf\n1\tInf\t3\n"
        ),
    )
    .unwrap();
}

#[test]
fn imports_acd_into_measurement_schema_without_normalizing_values() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("measurement.acd");
    let output = directory.path().join("measurement.h5");
    write_acd(&source, "time\twheel/travel\twheel travel");

    measurement_hdf5::import_acd(&source, &output, false).unwrap();

    let measurement = MeasurementFile::open(&output).unwrap();
    assert_eq!(measurement.summary().format_version, 1);
    assert_eq!(
        measurement.summary().source_file,
        source.display().to_string()
    );
    assert_eq!(measurement.summary().encoding.as_deref(), Some("utf-8-sig"));
    assert_eq!(measurement.summary().dialect.as_deref(), Some("ascii3"));
    assert_eq!(measurement.read_sample_axis().unwrap(), vec![0.0, 1.0]);
    let travel = measurement
        .read_channel("wheel/travel", Some("mm"))
        .unwrap();
    assert!(travel.values[0].is_nan());
    assert!(travel.values[1].is_infinite());
    assert_eq!(travel.info.group_name, "wheel_travel");
    assert_eq!(measurement.channels()[2].group_name, "wheel_travel_2");
    assert!(
        measurement
            .read_channel("wheel travel", None)
            .unwrap()
            .values[0]
            .is_infinite()
    );
    assert_eq!(measurement.test_info()[0].unit, "");
}

#[test]
fn rejects_acd_without_exact_time_channel() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("measurement.acd");
    let output = directory.path().join("measurement.h5");
    write_acd(&source, "Time\twheel/travel\twheel travel");

    let error = measurement_hdf5::import_acd(&source, &output, false).unwrap_err();
    assert!(error.to_string().contains("named exactly 'time'"));
}

#[test]
fn committed_vertical_measurement_example_imports() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/input/example-measurement-knc-vertical.acd");
    assert!(source.is_file(), "missing {}", source.display());
    let directory = tempdir().unwrap();
    let output = directory.path().join("example-vertical.h5");

    measurement_hdf5::import_acd(&source, &output, false).unwrap();
    let imported = MeasurementFile::open(output).unwrap();

    assert_eq!(imported.read_sample_axis().unwrap().len(), 6);
    assert_eq!(imported.channels().len(), 17);
    assert_eq!(imported.test_info().len(), 5);
}
