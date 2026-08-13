"""Canonical SVC simulation signal mapping."""

from knc_validation.config import SignalRef

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
        SignalRef("/results/til_wheel_tire_forces_2", "normal_rear", "newton"),
    ),
    (
        "rear_right_wheel_load",
        SignalRef("/results/tir_wheel_tire_forces_2", "normal_rear", "newton"),
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
)
