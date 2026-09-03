use std::path::Path;

use adams_res_converter::{hdf5_writer, parser};
use hdf5::types::VarLenUnicode;
use tempfile::tempdir;

#[test]
fn converts_a_result_file_to_hdf5() {
    let source = Path::new("tests/fixtures/minimal.res");
    let result = parser::parse_file(source).unwrap();
    let directory = tempdir().unwrap();
    let output = directory.path().join("minimal.h5");

    hdf5_writer::write_file(&output, source, &result).unwrap();

    let file = hdf5::File::open(output).unwrap();
    assert_eq!(
        file.attr("sample_count")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        2
    );

    assert_eq!(
        file.group("results").unwrap().member_names().unwrap().len(),
        2
    );
    assert_eq!(
        file.attr("signal_count")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        result.signal_count() as u64
    );

    let group = file.group("results/vehicle.ride_heights").unwrap();
    assert_eq!(
        group
            .attr("object_id")
            .unwrap()
            .read_scalar::<u64>()
            .unwrap(),
        42
    );

    let dataset = group.dataset("values").unwrap();
    assert_eq!(dataset.shape(), vec![2, 2]);
    assert_eq!(
        dataset.read_raw::<f64>().unwrap(),
        vec![361.0, 360.0, 360.5, 359.5]
    );
    assert_eq!(
        group
            .dataset("adams_ids")
            .unwrap()
            .read_raw::<u64>()
            .unwrap(),
        vec![2, 1]
    );

    let component_names = group
        .dataset("component_names")
        .unwrap()
        .read_raw::<VarLenUnicode>()
        .unwrap();
    assert_eq!(
        component_names
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>(),
        vec!["front_left", "front_right"]
    );

    let units = group
        .dataset("units")
        .unwrap()
        .read_raw::<VarLenUnicode>()
        .unwrap();
    assert_eq!(
        units.iter().map(|unit| unit.as_str()).collect::<Vec<_>>(),
        vec!["mm", "mm"]
    );

    let written_value_count = file
        .group("results")
        .unwrap()
        .member_names()
        .unwrap()
        .into_iter()
        .map(|name| {
            file.group(&format!("results/{name}"))
                .unwrap()
                .dataset("values")
                .unwrap()
                .shape()[1]
        })
        .sum::<usize>();
    assert_eq!(written_value_count, result.signal_count());
}
