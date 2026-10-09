use std::fs;
use std::path::PathBuf;

use clab_converter::measurement_hdf5;
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use hdf5::types::VarLenUnicode;
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

fn vertical() -> clab_converter::identity::Identity {
    clab_converter::identity::Identity::new("knc", Some("vertical")).unwrap()
}

#[test]
fn imports_acd_into_measurement_schema_without_normalizing_values() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("measurement.acd");
    let output = directory.path().join("measurement.h5");
    write_acd(&source, "time\twheel/travel\twheel travel");

    measurement_hdf5::import_acd(&source, &output, false, &vertical()).unwrap();

    let measurement = CanonicalFile::open(&output).unwrap();
    assert_eq!(measurement.summary().schema_version, 2);
    assert_eq!(measurement.summary().source_kind, SourceKind::Measurement);
    assert_eq!(
        measurement.summary().source_file,
        source.display().to_string()
    );
    assert_eq!(measurement.read_axis().unwrap(), vec![0.0, 1.0]);
    let raw = hdf5::File::open(&output).unwrap();
    let axis = raw.group("axis").unwrap();
    assert_eq!(
        axis.attr("kind")
            .unwrap()
            .read_scalar::<VarLenUnicode>()
            .unwrap()
            .as_str(),
        "time"
    );
    assert_eq!(
        axis.dataset("values").unwrap().read_raw::<f64>().unwrap(),
        vec![0.0, 1.0]
    );
    let travel = measurement
        .read_signal("wheel/travel", None, Some("mm"))
        .unwrap();
    assert!(travel.values[0].is_nan());
    assert!(travel.values[1].is_infinite());
    assert_eq!(travel.info.safe_name, "wheel_travel");
    assert_eq!(measurement.signals()[2].safe_name, "wheel_travel_2");
    assert!(
        measurement
            .read_signal("wheel travel", None, None)
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

    let error = measurement_hdf5::import_acd(&source, &output, false, &vertical()).unwrap_err();
    assert!(error.to_string().contains("named exactly 'time'"));
}

#[test]
fn committed_vertical_measurement_example_imports() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/input/example-measurement-knc-vertical.acd");
    assert!(source.is_file(), "missing {}", source.display());
    let directory = tempdir().unwrap();
    let output = directory.path().join("example-vertical.h5");

    measurement_hdf5::import_acd(&source, &output, false, &vertical()).unwrap();
    let imported = CanonicalFile::open(output).unwrap();

    assert_eq!(imported.read_axis().unwrap().len(), 4096);
    assert_eq!(imported.signals().len(), 37);
    assert_eq!(imported.test_info().len(), 5);
}
