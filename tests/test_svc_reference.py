import json
from pathlib import Path

import pytest

from chassis_lab.svc_reference import (
    SvcReferenceError,
    load_svc_reference,
)


def reference_data() -> dict[str, object]:
    return {
        "schema_version": 1,
        "case": {
            "name": "example-config",
            "variant": "EXAMPLE",
            "loadcase": "2UP",
        },
        "coordinate_system": {
            "cg_x_origin": "front_axle_wheel_center",
            "cg_x_positive": "rearward",
        },
        "sources": [
            {
                "file": "source.txt",
                "sha256": "a" * 64,
                "test_date": "2019-09-06",
                "result": "reviewed",
            }
        ],
        "values": {
            "front_left_mass_kg": 413,
            "front_right_mass_kg": 389,
            "rear_left_mass_kg": 347,
            "rear_right_mass_kg": 351,
            "front_mass_kg": 802,
            "rear_mass_kg": 698,
            "total_mass_kg": 1500,
            "front_distribution_percent": 53.5,
            "rear_distribution_percent": 46.5,
            "wheelbase_mm": 2497,
            "cg_x_from_front_axle_mm": 1161.9373,
        },
    }


def write_reference(tmp_path: Path, data: object) -> Path:
    path = tmp_path / "reference.json"
    path.write_text(json.dumps(data), encoding="utf-8")
    return path


def test_loads_svc_reference(tmp_path: Path) -> None:
    path = write_reference(tmp_path, reference_data())

    reference = load_svc_reference(path)

    assert reference.path == path.resolve()
    assert reference.schema_version == 1
    assert reference.case.name == "example-config"
    assert reference.coordinate_system.cg_x_positive == "rearward"
    assert reference.sources[0].file == "source.txt"
    assert reference.value("total_mass_kg") == 1500.0
    with pytest.raises(KeyError):
        reference.value("missing")


def test_allows_subset_of_canonical_metrics(tmp_path: Path) -> None:
    data = reference_data()
    data["values"] = {"total_mass_kg": 1500}

    reference = load_svc_reference(write_reference(tmp_path, data))

    assert reference.values == (("total_mass_kg", 1500.0),)


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("schema_version", 2, "unsupported"),
        ("schema_version", True, "unsupported"),
        ("sources", [], "non-empty array"),
        ("values", {}, "at least one metric"),
    ],
)
def test_rejects_invalid_top_level_values(
    tmp_path: Path, field: str, value: object, message: str
) -> None:
    data = reference_data()
    data[field] = value

    with pytest.raises(SvcReferenceError, match=message):
        load_svc_reference(write_reference(tmp_path, data))


def test_rejects_invalid_json_and_non_object_root(tmp_path: Path) -> None:
    path = tmp_path / "reference.json"
    path.write_text("{", encoding="utf-8")
    with pytest.raises(SvcReferenceError, match="invalid JSON"):
        load_svc_reference(path)

    path.write_text("[]", encoding="utf-8")
    with pytest.raises(SvcReferenceError, match="root must be an object"):
        load_svc_reference(path)


def test_rejects_unknown_keys_and_metrics(tmp_path: Path) -> None:
    data = reference_data()
    data["extra"] = True
    with pytest.raises(SvcReferenceError, match="unknown SVC reference key"):
        load_svc_reference(write_reference(tmp_path, data))

    data = reference_data()
    data["values"] = {"wheel_base_mm": 2497}
    with pytest.raises(SvcReferenceError, match="unknown SVC reference metric"):
        load_svc_reference(write_reference(tmp_path, data))


@pytest.mark.parametrize("checksum", ["...", "A" * 64, "g" * 64])
def test_rejects_invalid_source_checksum(tmp_path: Path, checksum: str) -> None:
    data = reference_data()
    data["sources"][0]["sha256"] = checksum

    with pytest.raises(SvcReferenceError, match="SHA-256"):
        load_svc_reference(write_reference(tmp_path, data))


def test_rejects_invalid_source_date_and_duplicates(tmp_path: Path) -> None:
    data = reference_data()
    data["sources"][0]["test_date"] = "06/09/2019"
    with pytest.raises(SvcReferenceError, match="YYYY-MM-DD"):
        load_svc_reference(write_reference(tmp_path, data))

    data = reference_data()
    data["sources"].append(dict(data["sources"][0]))
    with pytest.raises(SvcReferenceError, match="duplicate reference source"):
        load_svc_reference(write_reference(tmp_path, data))


@pytest.mark.parametrize("value", [True, "1500", float("nan")])
def test_rejects_non_numeric_or_nonfinite_metric(
    tmp_path: Path, value: object
) -> None:
    data = reference_data()
    data["values"] = {"total_mass_kg": value}

    with pytest.raises(SvcReferenceError, match="must be"):
        load_svc_reference(write_reference(tmp_path, data))


@pytest.mark.parametrize(
    ("metric", "value", "message"),
    [
        ("front_mass_kg", 803, "front_mass_kg"),
        ("total_mass_kg", 1501, "total_mass_kg"),
        ("front_distribution_percent", 53.0, "distribution"),
        ("cg_x_from_front_axle_mm", 1100, "cg_x"),
    ],
)
def test_rejects_inconsistent_derived_values(
    tmp_path: Path, metric: str, value: float, message: str
) -> None:
    data = reference_data()
    data["values"][metric] = value

    with pytest.raises(SvcReferenceError, match=message):
        load_svc_reference(write_reference(tmp_path, data))
