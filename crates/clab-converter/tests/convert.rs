use clab_converter::{hdf5_writer, parser};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use hdf5::types::VarLenUnicode;
use tempfile::tempdir;

mod common;

#[test]
fn converts_a_result_file_to_hdf5() {
    let directory = tempdir().unwrap();
    let source = directory.path().join("minimal.res");
    std::fs::write(&source, common::MINIMAL_RES).unwrap();
    let result = parser::parse_file(&source).unwrap();
    let output = directory.path().join("minimal.h5");

    hdf5_writer::write_file(&output, &source, &result).unwrap();

    let file = hdf5::File::open(&output).unwrap();
    assert_eq!(
        file.attr("sample_count")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        2
    );

    assert_eq!(
        file.attr("schema_version")
            .unwrap()
            .read_scalar::<u32>()
            .unwrap(),
        1
    );
    assert_eq!(
        file.attr("source_kind")
            .unwrap()
            .read_scalar::<VarLenUnicode>()
            .unwrap()
            .as_str(),
        "simulation"
    );
    assert_eq!(
        file.attr("signal_count")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        result.signal_count() as u64
    );

    let group = file
        .group("signals/vehicle_ride_heights_front_left")
        .unwrap();
    assert_eq!(
        group
            .attr("object_id")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        42
    );

    let dataset = group.dataset("values").unwrap();
    assert_eq!(dataset.shape(), vec![2]);
    assert_eq!(dataset.read_raw::<f64>().unwrap(), vec![361.0, 360.5]);
    assert_eq!(
        group
            .attr("adams_id")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        2
    );

    assert_eq!(
        file.group("signals").unwrap().member_names().unwrap().len(),
        result.signal_count()
    );

    let axis = file.group("axis").unwrap();
    assert_eq!(
        axis.attr("kind")
            .unwrap()
            .read_scalar::<VarLenUnicode>()
            .unwrap()
            .as_str(),
        "sample_index"
    );
    assert_eq!(
        axis.dataset("values").unwrap().read_raw::<u64>().unwrap(),
        vec![0, 1]
    );
    drop(axis);
    drop(file);

    let canonical = CanonicalFile::open(&output).unwrap();
    assert_eq!(canonical.summary().source_kind, SourceKind::Simulation);
    assert_eq!(canonical.read_axis().unwrap(), vec![0.0, 1.0]);
}
