"""Canonical SVC signal conventions."""

from chassis_lab.conventions import BaselinePolicy, Corner, SignalConvention

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
