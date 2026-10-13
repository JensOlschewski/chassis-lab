//! Canonical units and signal transformations owned by Rust.

use std::{collections::BTreeMap, f64::consts::PI, fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unit {
    Newton,
    Kilogram,
    Millimetre,
    Metre,
    Radian,
    Degree,
    NewtonMillimetre,
    NewtonMetre,
    KilogramMillimetreSquared,
}

impl Unit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Newton => "newton",
            Self::Kilogram => "kg",
            Self::Millimetre => "mm",
            Self::Metre => "m",
            Self::Radian => "rad",
            Self::Degree => "deg",
            Self::NewtonMillimetre => "newton-mm",
            Self::NewtonMetre => "newton-meter",
            Self::KilogramMillimetreSquared => "kg*mm^2",
        }
    }
}

impl FromStr for Unit {
    type Err = ConventionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "newton" => Ok(Self::Newton),
            "kg" => Ok(Self::Kilogram),
            "mm" => Ok(Self::Millimetre),
            "m" => Ok(Self::Metre),
            "rad" => Ok(Self::Radian),
            "deg" => Ok(Self::Degree),
            "newton-mm" => Ok(Self::NewtonMillimetre),
            "newton-meter" => Ok(Self::NewtonMetre),
            "kg*mm^2" => Ok(Self::KilogramMillimetreSquared),
            _ => Err(ConventionError(format!("unsupported unit: '{value}'"))),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

pub const SVC_METRICS: [(&str, &str); 29] = [
    ("front_left_mass_kg", "kg"),
    ("front_right_mass_kg", "kg"),
    ("rear_left_mass_kg", "kg"),
    ("rear_right_mass_kg", "kg"),
    ("front_mass_kg", "kg"),
    ("rear_mass_kg", "kg"),
    ("total_mass_kg", "kg"),
    ("front_distribution_percent", "%"),
    ("rear_distribution_percent", "%"),
    ("wheelbase_mm", "mm"),
    ("cg_x_from_front_axle_mm", "mm"),
    ("cg_height_mm", "mm"),
    ("ride_height_front_left_mm", "mm"),
    ("ride_height_front_right_mm", "mm"),
    ("ride_height_rear_left_mm", "mm"),
    ("ride_height_rear_right_mm", "mm"),
    ("front_track_mm", "mm"),
    ("rear_track_mm", "mm"),
    ("toe_front_left_deg", "deg"),
    ("toe_front_right_deg", "deg"),
    ("toe_rear_left_deg", "deg"),
    ("toe_rear_right_deg", "deg"),
    ("camber_front_left_deg", "deg"),
    ("camber_front_right_deg", "deg"),
    ("camber_rear_left_deg", "deg"),
    ("camber_rear_right_deg", "deg"),
    ("inertia_xx_kg_mm2", "kg*mm^2"),
    ("inertia_yy_kg_mm2", "kg*mm^2"),
    ("inertia_zz_kg_mm2", "kg*mm^2"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Corner {
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
}

impl Corner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FrontLeft => "front_left",
            Self::FrontRight => "front_right",
            Self::RearLeft => "rear_left",
            Self::RearRight => "rear_right",
        }
    }

    pub const fn all() -> [Self; 4] {
        [
            Self::FrontLeft,
            Self::FrontRight,
            Self::RearLeft,
            Self::RearRight,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaselinePolicy {
    Absolute,
    MeasurementOffset,
    StaticSimulation,
}

impl BaselinePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::MeasurementOffset => "measurement_offset",
            Self::StaticSimulation => "static_simulation",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignalConvention {
    pub unit: Unit,
    pub source_unit: String,
    pub scale: f64,
    pub sign: i8,
    pub corner: Option<Corner>,
    pub baseline: BaselinePolicy,
}

impl SignalConvention {
    pub fn new(
        unit: Unit,
        source_unit: impl Into<String>,
        scale: f64,
        sign: i8,
        corner: Option<Corner>,
        baseline: BaselinePolicy,
    ) -> Result<Self, ConventionError> {
        if !matches!(sign, -1 | 1) {
            return Err(ConventionError("sign must be either 1 or -1".into()));
        }
        if !scale.is_finite() || scale == 0.0 {
            return Err(ConventionError("scale must be finite and non-zero".into()));
        }
        Ok(Self {
            unit,
            source_unit: source_unit.into(),
            scale,
            sign,
            corner,
            baseline,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransformedSignal {
    pub name: String,
    pub values: Vec<f64>,
    pub unit: Unit,
    pub source: String,
    pub original_unit: String,
    pub conversion_factor: f64,
    pub convention: SignalConvention,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ConventionError(pub String);

pub fn convert_values(
    values: &[f64],
    source_unit: &str,
    target_unit: Unit,
    gravity: f64,
) -> Result<Vec<f64>, ConventionError> {
    if source_unit.is_empty() {
        return Err(ConventionError(format!(
            "cannot convert to '{target_unit}' without source unit metadata"
        )));
    }
    if !gravity.is_finite() || gravity <= 0.0 {
        return Err(ConventionError(
            "gravity must be finite and positive".into(),
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ConventionError("signal values must be finite".into()));
    }
    let factor = conversion_factor(source_unit, target_unit, gravity)?;
    Ok(values.iter().map(|value| value * factor).collect())
}

/// Convert finite source values, then apply configured sign and scale.
///
/// Conversion and gravity validation happen before sign/scale transformation;
/// returned metadata records both source and canonical units.
pub fn apply_convention(
    values: &[f64],
    name: impl Into<String>,
    source: impl Into<String>,
    convention: SignalConvention,
    gravity: f64,
) -> Result<TransformedSignal, ConventionError> {
    if !gravity.is_finite() || gravity <= 0.0 {
        return Err(ConventionError(
            "gravity must be finite and positive".into(),
        ));
    }
    let factor = conversion_factor(&convention.source_unit, convention.unit, gravity)?;
    let converted = convert_values(values, &convention.source_unit, convention.unit, gravity)?;
    let multiplier = f64::from(convention.sign) * convention.scale;
    let transformed = converted
        .into_iter()
        .map(|value| value * multiplier)
        .collect();
    Ok(TransformedSignal {
        name: name.into(),
        values: transformed,
        unit: convention.unit,
        source: source.into(),
        original_unit: convention.source_unit.clone(),
        conversion_factor: factor,
        convention,
    })
}

/// Apply canonical conversion while leaving non-finite non-selected samples intact.
/// SVC selects and validates only final sample, matching legacy evaluator behavior.
pub fn apply_convention_for_final_sample(
    values: &[f64],
    name: impl Into<String>,
    source: impl Into<String>,
    convention: SignalConvention,
    gravity: f64,
) -> Result<TransformedSignal, ConventionError> {
    if !gravity.is_finite() || gravity <= 0.0 {
        return Err(ConventionError(
            "gravity must be finite and positive".into(),
        ));
    }
    let factor = conversion_factor(&convention.source_unit, convention.unit, gravity)?;
    let multiplier = f64::from(convention.sign) * convention.scale;
    let transformed = values
        .iter()
        .map(|value| value * factor * multiplier)
        .collect();
    Ok(TransformedSignal {
        name: name.into(),
        values: transformed,
        unit: convention.unit,
        source: source.into(),
        original_unit: convention.source_unit.clone(),
        conversion_factor: factor,
        convention,
    })
}

fn conversion_factor(
    source_unit: &str,
    target_unit: Unit,
    gravity: f64,
) -> Result<f64, ConventionError> {
    let source = Unit::from_str(source_unit)?;
    if source == target_unit {
        return Ok(1.0);
    }
    match (source, target_unit) {
        (Unit::Newton, Unit::Kilogram) => Ok(1.0 / gravity),
        (Unit::Kilogram, Unit::Newton) => Ok(gravity),
        (Unit::Millimetre, Unit::Metre) => Ok(0.001),
        (Unit::Metre, Unit::Millimetre) => Ok(1000.0),
        (Unit::Radian, Unit::Degree) => Ok(180.0 / PI),
        (Unit::Degree, Unit::Radian) => Ok(PI / 180.0),
        (Unit::NewtonMillimetre, Unit::NewtonMetre) => Ok(0.001),
        (Unit::NewtonMetre, Unit::NewtonMillimetre) => Ok(1000.0),
        _ => Err(ConventionError(format!(
            "unsupported conversion: '{source_unit}' -> '{target_unit}'"
        ))),
    }
}

pub fn svc_conventions() -> BTreeMap<String, SignalConvention> {
    let mut conventions = BTreeMap::new();
    for (name, unit, corner) in [
        (
            "front_left_wheel_load",
            Unit::Kilogram,
            Some(Corner::FrontLeft),
        ),
        (
            "front_right_wheel_load",
            Unit::Kilogram,
            Some(Corner::FrontRight),
        ),
        (
            "rear_left_wheel_load",
            Unit::Kilogram,
            Some(Corner::RearLeft),
        ),
        (
            "rear_right_wheel_load",
            Unit::Kilogram,
            Some(Corner::RearRight),
        ),
    ] {
        conventions.insert(
            name.into(),
            SignalConvention::new(
                unit,
                if unit == Unit::Kilogram {
                    "newton"
                } else {
                    unit.as_str()
                },
                1.0,
                1,
                corner,
                BaselinePolicy::StaticSimulation,
            )
            .expect("canonical convention is valid"),
        );
    }
    for corner in Corner::all() {
        let prefix = corner.as_str();
        for suffix in ["ride_height", "toe", "camber"] {
            let unit = if suffix == "ride_height" {
                Unit::Millimetre
            } else {
                Unit::Degree
            };
            conventions.insert(
                format!("{prefix}_{suffix}"),
                SignalConvention::new(
                    unit,
                    unit.as_str(),
                    1.0,
                    1,
                    Some(corner),
                    BaselinePolicy::StaticSimulation,
                )
                .expect("canonical convention is valid"),
            );
        }
    }
    for name in [
        "wheelbase",
        "front_track",
        "rear_track",
        "cg_height",
        "gyro_x",
    ] {
        conventions.insert(
            name.into(),
            SignalConvention::new(
                Unit::Millimetre,
                "mm",
                1.0,
                1,
                None,
                BaselinePolicy::StaticSimulation,
            )
            .expect("canonical convention is valid"),
        );
    }
    for name in ["inertia_xx", "inertia_yy", "inertia_zz"] {
        conventions.insert(
            name.into(),
            SignalConvention::new(
                Unit::KilogramMillimetreSquared,
                "kg*mm^2",
                1.0,
                1,
                None,
                BaselinePolicy::StaticSimulation,
            )
            .expect("canonical convention is valid"),
        );
    }
    conventions.insert(
        "front_left_wheel_center_x".into(),
        SignalConvention::new(
            Unit::Millimetre,
            "mm",
            1.0,
            1,
            None,
            BaselinePolicy::StaticSimulation,
        )
        .expect("canonical convention is valid"),
    );
    conventions
}

pub fn vertical_conventions() -> BTreeMap<String, SignalConvention> {
    let mut conventions = BTreeMap::new();
    for corner in Corner::all() {
        for suffix in [
            "travel",
            "wheel_center_x",
            "wheel_center_y",
            "wheel_center_z",
            "tire_deflection",
        ] {
            conventions.insert(
                format!("{}_{}", corner.as_str(), suffix),
                SignalConvention::new(
                    Unit::Millimetre,
                    "mm",
                    1.0,
                    1,
                    Some(corner),
                    BaselinePolicy::StaticSimulation,
                )
                .expect("canonical convention is valid"),
            );
        }
        for (suffix, unit, sign) in [
            ("wheel_load", Unit::Newton, 1),
            (
                "toe",
                Unit::Degree,
                if matches!(corner, Corner::FrontRight | Corner::RearRight) {
                    -1
                } else {
                    1
                },
            ),
            ("camber", Unit::Degree, 1),
            // Hub rotation about Y relative to the body; same sign on both sides.
            ("spin", Unit::Degree, 1),
        ] {
            conventions.insert(
                format!("{}_{}", corner.as_str(), suffix),
                SignalConvention::new(
                    unit,
                    unit.as_str(),
                    1.0,
                    sign,
                    Some(corner),
                    BaselinePolicy::StaticSimulation,
                )
                .expect("canonical convention is valid"),
            );
        }
    }
    conventions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_units_without_rounding() {
        assert_eq!(
            convert_values(&[9.80665, 19.6133], "newton", Unit::Kilogram, 9.80665).unwrap(),
            [1.0, 2.0]
        );
        assert_eq!(
            convert_values(&[1000.0], "mm", Unit::Metre, 9.80665).unwrap(),
            [1.0]
        );
        assert!(
            (convert_values(&[PI], "rad", Unit::Degree, 9.80665).unwrap()[0] - 180.0).abs() < 1e-12
        );
    }

    #[test]
    fn rejects_missing_units_and_invalid_conventions() {
        assert!(
            convert_values(&[1.0], "", Unit::Millimetre, 9.80665)
                .unwrap_err()
                .to_string()
                .contains("without source unit")
        );
        assert!(
            SignalConvention::new(
                Unit::Millimetre,
                "mm",
                0.0,
                1,
                None,
                BaselinePolicy::Absolute,
            )
            .is_err()
        );
        assert!(convert_values(&[f64::NAN], "mm", Unit::Millimetre, 9.80665).is_err());
    }

    #[test]
    fn applies_scale_sign_and_provenance() {
        let convention = SignalConvention::new(
            Unit::Degree,
            "rad",
            2.0,
            -1,
            Some(Corner::FrontRight),
            BaselinePolicy::MeasurementOffset,
        )
        .unwrap();
        let signal = apply_convention(
            &[PI / 2.0],
            "front_right_toe",
            "measurement.acd/toe_fr",
            convention,
            9.80665,
        )
        .unwrap();
        assert_eq!(signal.values, [-180.0]);
        assert_eq!(signal.original_unit, "rad");
        assert_eq!(signal.convention.corner, Some(Corner::FrontRight));
    }

    #[test]
    fn final_sample_conversion_preserves_unselected_nonfinite_values() {
        let convention = SignalConvention::new(
            Unit::Kilogram,
            "newton",
            1.0,
            1,
            Some(Corner::FrontLeft),
            BaselinePolicy::StaticSimulation,
        )
        .unwrap();
        let signal = apply_convention_for_final_sample(
            &[f64::NAN, 9.80665],
            "front_left_wheel_load",
            "simulation.h5",
            convention,
            9.80665,
        )
        .unwrap();
        assert!(signal.values[0].is_nan());
        assert_eq!(signal.values[1], 1.0);
    }

    #[test]
    fn canonical_mappings_cover_svc_and_vertical_signals() {
        assert!(svc_conventions().contains_key("front_left_wheel_load"));
        assert_eq!(SVC_METRICS.len(), 29);
        assert_eq!(SVC_METRICS[0], ("front_left_mass_kg", "kg"));
        assert_eq!(SVC_METRICS[28], ("inertia_zz_kg_mm2", "kg*mm^2"));
        assert_eq!(vertical_conventions().len(), 36);
        assert_eq!(vertical_conventions()["front_right_toe"].sign, -1);
        assert_eq!(vertical_conventions()["front_right_spin"].sign, 1);
    }
}
