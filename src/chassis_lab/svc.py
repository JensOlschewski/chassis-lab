"""Canonical SVC simulation signal mapping."""

from chassis_lab.config import SignalRef
from chassis_lab.conventions import (
    BaselinePolicy,
    Corner,
    SignalConvention,
)

SVC_SIGNAL_MAPPING: tuple[tuple[str, SignalRef], ...] = (
    (
        "front_left_wheel_load",
        SignalRef("til_wheel_tire_forces", "normal_front", "newton"),
    ),
    (
        "front_right_wheel_load",
        SignalRef("tir_wheel_tire_forces", "normal_front", "newton"),
    ),
    (
        "rear_left_wheel_load",
        SignalRef("til_wheel_tire_forces", "normal_rear", "newton"),
    ),
    (
        "rear_right_wheel_load",
        SignalRef("tir_wheel_tire_forces", "normal_rear", "newton"),
    ),
    (
        "front_left_ride_height",
        SignalRef("ride_heights", "ride_height_front_left", "mm"),
    ),
    (
        "front_right_ride_height",
        SignalRef("ride_heights", "ride_height_front_right", "mm"),
    ),
    (
        "rear_left_ride_height",
        SignalRef("ride_heights", "ride_height_rear_left", "mm"),
    ),
    (
        "rear_right_ride_height",
        SignalRef("ride_heights", "ride_height_rear_right", "mm"),
    ),
    ("wheelbase", SignalRef("svc_output", "Wheelbase", "mm")),
    ("front_track", SignalRef("svc_output", "front_track", "mm")),
    ("rear_track", SignalRef("svc_output", "rear_track", "mm")),
    ("cg_height", SignalRef("svc_output", "CG_height", "mm")),
    ("inertia_xx", SignalRef("svc_output", "IXX", "kg*mm^2")),
    ("inertia_yy", SignalRef("svc_output", "IYY", "kg*mm^2")),
    ("inertia_zz", SignalRef("svc_output", "IZZ", "kg*mm^2")),
    ("gyro_x", SignalRef("testrig.ges_gyro", "X", "mm")),
    (
        "front_left_wheel_center_x",
        SignalRef("B479_FDJ3_16z_front_whl.whl_wheel", "X", "mm"),
    ),
    (
        "front_left_toe",
        SignalRef("toe_camber_body", "left_toe_angle_front", "deg"),
    ),
    (
        "front_right_toe",
        SignalRef("toe_camber_body", "right_toe_angle_front", "deg"),
    ),
    (
        "rear_left_toe",
        SignalRef("toe_camber_body", "left_toe_angle_rear", "deg"),
    ),
    (
        "rear_right_toe",
        SignalRef("toe_camber_body", "right_toe_angle_rear", "deg"),
    ),
    (
        "front_left_camber",
        SignalRef("toe_camber_body", "left_camber_angle_front", "deg"),
    ),
    (
        "front_right_camber",
        SignalRef("toe_camber_body", "right_camber_angle_front", "deg"),
    ),
    (
        "rear_left_camber",
        SignalRef("toe_camber_body", "left_camber_angle_rear", "deg"),
    ),
    (
        "rear_right_camber",
        SignalRef("toe_camber_body", "right_camber_angle_rear", "deg"),
    ),
)


SVC_SIGNAL_CONVENTIONS: dict[str, SignalConvention] = {
    "front_left_wheel_load": SignalConvention(
        unit="kg",
        source_unit="newton",
        corner=Corner.FRONT_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_right_wheel_load": SignalConvention(
        unit="kg",
        source_unit="newton",
        corner=Corner.FRONT_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_left_wheel_load": SignalConvention(
        unit="kg",
        source_unit="newton",
        corner=Corner.REAR_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_right_wheel_load": SignalConvention(
        unit="kg",
        source_unit="newton",
        corner=Corner.REAR_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_left_ride_height": SignalConvention(
        unit="mm",
        source_unit="mm",
        corner=Corner.FRONT_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_right_ride_height": SignalConvention(
        unit="mm",
        source_unit="mm",
        corner=Corner.FRONT_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_left_ride_height": SignalConvention(
        unit="mm",
        source_unit="mm",
        corner=Corner.REAR_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_right_ride_height": SignalConvention(
        unit="mm",
        source_unit="mm",
        corner=Corner.REAR_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "wheelbase": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "front_track": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "rear_track": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "cg_height": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "inertia_xx": SignalConvention(
        unit="kg*mm^2",
        source_unit="kg*mm^2",
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "inertia_yy": SignalConvention(
        unit="kg*mm^2",
        source_unit="kg*mm^2",
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "inertia_zz": SignalConvention(
        unit="kg*mm^2",
        source_unit="kg*mm^2",
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "gyro_x": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "front_left_wheel_center_x": SignalConvention(
        unit="mm", source_unit="mm", baseline=BaselinePolicy.STATIC_SIMULATION
    ),
    "front_left_toe": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.FRONT_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_right_toe": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.FRONT_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_left_toe": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.REAR_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_right_toe": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.REAR_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_left_camber": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.FRONT_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "front_right_camber": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.FRONT_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_left_camber": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.REAR_LEFT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
    "rear_right_camber": SignalConvention(
        unit="deg",
        source_unit="deg",
        corner=Corner.REAR_RIGHT,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    ),
}
