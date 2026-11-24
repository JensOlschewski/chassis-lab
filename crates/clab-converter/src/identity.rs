//! Test and subtest a converted file belongs to.
//!
//! The caller declares both. The data cross-checks the declaration before
//! anything is written: a declared test or subtest the data contradicts is
//! a hard error, not a warning, so a canonical HDF5 file is never written
//! under the wrong identity.

use anyhow::{Result, bail};

use crate::acd::AcdMeasurement;
use crate::conditions::Conditions;
use crate::model::AdamsResult;

/// Known tests and their subtests. Add new tests and subtests here.
pub const TESTS: &[(&str, &[&str])] = &[
    ("svc", &[]),
    (
        "knc",
        &["vertical", "roll", "static", "lateral", "longitudinal"],
    ),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub test: String,
    pub subtest: Option<String>,
    /// Declared KnC setup; stored next to test and subtest.
    pub conditions: Option<Conditions>,
}

impl Identity {
    /// Validate a declared test and subtest against the known tests.
    pub fn new(test: &str, subtest: Option<&str>) -> Result<Self> {
        let known = |names: &[&str]| names.join(", ");
        let Some((_, tests)) = TESTS.iter().find(|(name, _)| *name == test) else {
            let tests: Vec<_> = TESTS.iter().map(|(name, _)| *name).collect();
            bail!("unknown test '{test}'; expected one of: {}", known(&tests));
        };
        match (tests.is_empty(), subtest) {
            (true, Some(subtest)) => {
                bail!("test '{test}' has no subtests; remove --subtest {subtest}")
            }
            (false, None) => bail!(
                "test '{test}' requires --subtest; expected one of: {}",
                known(tests)
            ),
            (false, Some(subtest)) if !tests.contains(&subtest) => bail!(
                "unknown subtest '{subtest}' for test '{test}'; expected one of: {}",
                known(tests)
            ),
            _ => Ok(Self {
                test: test.into(),
                subtest: subtest.map(Into::into),
                conditions: None,
            }),
        }
    }
}

/// Test an Adams result indicates through its `analysis_type` variable:
/// 0 is a KnC rig analysis, 1 an SVC analysis. The last sample is used,
/// because SVC results start at 0.
pub fn res_test(result: &AdamsResult) -> Option<&'static str> {
    let value = result
        .values()
        .find(|(entity, component)| {
            component.name == "Q"
                && (entity.name == "analysis_type"
                    || entity
                        .source_path
                        .as_deref()
                        .is_some_and(|path| path.ends_with(".analysis_type")))
        })?
        .1
        .values
        .last()
        .copied()?;
    match value {
        0.0 => Some("knc"),
        1.0 => Some("svc"),
        _ => None,
    }
}

/// KnC when the result has a `wheel_travel` entity; SVC results never do.
fn res_structural_test(result: &AdamsResult) -> Option<&'static str> {
    result
        .entities
        .iter()
        .any(|entity| entity.name.contains("wheel_travel"))
        .then_some("knc")
}

/// Warnings when the `analysis_type` value or a `wheel_travel` entity
/// contradicts the declared test, or when neither signal exists.
pub fn res_test_warnings(result: &AdamsResult, declared: &str) -> Vec<String> {
    let signals = [
        ("analysis_type", res_test(result)),
        ("wheel_travel entity", res_structural_test(result)),
    ];
    if signals.iter().all(|(_, found)| found.is_none()) {
        return vec!["no analysis_type or wheel_travel, test not verified".into()];
    }
    signals
        .into_iter()
        .filter_map(|(signal, found)| {
            found
                .filter(|found| *found != declared)
                .map(|found| format!("{signal} indicates '{found}', but --test is '{declared}'"))
        })
        .collect()
}

/// Roll motion range (rad) above which the roll rig moves.
const ROLL_MOVES_RAD: f64 = 0.01;
/// Vertical motion range (mm) above which the vertical rig moves.
/// Roll leaks up to about 12 mm into vertical, so stay above that.
const VERTICAL_MOVES_MM: f64 = 15.0;

/// Test the motion data of a KnC result indicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    Test(&'static str),
    /// Motion data is ambiguous, for example a combined file.
    Undetermined,
    /// No `motion_roll` or `motion_vertical` channels.
    Unavailable,
}

/// Applied wheel force range (N) above which a compliance subtest drives the wheels.
/// Measured: compliance runs apply 3000 N, all other runs exactly 0.
const FORCE_MOVES_N: f64 = 100.0;
/// Applied wheel torque range (N·mm) above which an aligning subtest runs.
const TORQUE_MOVES_NMM: f64 = 10_000.0;
/// Steering input range (rad) above which a steering subtest runs.
const STEER_MOVES_RAD: f64 = 0.05;

/// Largest peak-to-peak range over the named component of every entity
/// whose name satisfies `matches`. `None` when no such component exists.
fn component_range(
    result: &AdamsResult,
    matches: impl Fn(&str) -> bool,
    component_name: Option<&str>,
) -> Option<f64> {
    result
        .entities
        .iter()
        .filter(|entity| matches(&entity.name))
        .flat_map(|entity| &entity.components)
        .filter(|component| component_name.is_none_or(|name| component.name == name))
        .map(|component| {
            let (min, max) = component
                .values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                });
            if max >= min { max - min } else { 0.0 }
        })
        .reduce(f64::max)
}

/// Largest peak-to-peak range over all components of the named entity.
fn motion_range(result: &AdamsResult, entity_name: &str) -> Option<f64> {
    component_range(result, |name| name == entity_name, None)
}

/// Largest range of one component over the four wheel force entities.
/// A missing entity means that nothing is applied.
fn wheel_force_range(result: &AdamsResult, component: &str) -> f64 {
    component_range(
        result,
        |name| name.starts_with("gforce_tire_"),
        Some(component),
    )
    .unwrap_or(0.0)
}

/// Detect the KnC subtest from which rig motion moves and which wheel
/// loads are applied. Several excitations at once, or an excitation
/// without its own subtest (steering, aligning torque), are undetermined.
pub fn res_subtest(result: &AdamsResult) -> Detection {
    let (Some(roll), Some(vertical)) = (
        motion_range(result, "motion_roll"),
        motion_range(result, "motion_vertical"),
    ) else {
        return Detection::Unavailable;
    };
    let steer = component_range(
        result,
        |name| name == "steering_wheel_input",
        Some("steering_wheel_input"),
    )
    .unwrap_or(0.0);
    let lateral = wheel_force_range(result, "FY") > FORCE_MOVES_N;
    // Torque next to lateral force is the pneumatic trail offset (Tz = Fy × offset).
    let aligning = !lateral && wheel_force_range(result, "TZ") > TORQUE_MOVES_NMM;
    let excited = [
        (roll > ROLL_MOVES_RAD, Some("roll")),
        (vertical > VERTICAL_MOVES_MM, Some("vertical")),
        (
            wheel_force_range(result, "FX") > FORCE_MOVES_N,
            Some("longitudinal"),
        ),
        (lateral, Some("lateral")),
        (aligning, None),
        (steer > STEER_MOVES_RAD, None),
    ];
    let mut active = excited.iter().filter(|(moves, _)| *moves);
    match (active.next(), active.next()) {
        (None, _) => Detection::Test("static"),
        (Some((_, Some(subtest))), None) => Detection::Test(subtest),
        _ => Detection::Undetermined,
    }
}

/// Front wheel-centre Z range (mm) above which heave or roll counts as moving.
/// Measured: roll leaks about 11 mm into heave, vertical about 2 mm into roll.
const ACD_MOVES_MM: f64 = 25.0;

fn acd_channel<'a>(measurement: &'a AcdMeasurement, name: &str) -> Option<&'a [f64]> {
    let channel = measurement.channels.iter().find(|c| c.name == name)?;
    Some(&channel.values)
}

/// Peak-to-peak range of `f(left, right)` over the finite samples.
fn paired_range(left: &[f64], right: &[f64], f: impl Fn(f64, f64) -> f64) -> f64 {
    let (min, max) = left
        .iter()
        .zip(right)
        .map(|(l, r)| f(*l, *r))
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    if max >= min { max - min } else { 0.0 }
}

/// Front wheel force range (N) above which a compliance subtest drives the wheels.
/// Measured: compliance runs span 6000 N and more, all other runs at most 156 N.
const ACD_FORCE_MOVES_N: f64 = 1000.0;

/// Largest range of the named force channels.
fn force_range(measurement: &AcdMeasurement, names: [&str; 2]) -> f64 {
    names
        .iter()
        .filter_map(|name| acd_channel(measurement, name))
        .map(|values| paired_range(values, values, |v, _| v))
        .fold(0.0, f64::max)
}

/// Detect the KnC subtest from front wheel forces and wheel-centre Z:
/// forces drive a compliance subtest, vertical moves both wheels together
/// (heave), roll moves them in opposition.
pub fn acd_motion_subtest(measurement: &AcdMeasurement) -> Detection {
    let (Some(left), Some(right)) = (
        acd_channel(measurement, "lfwcz"),
        acd_channel(measurement, "rfwcz"),
    ) else {
        return Detection::Unavailable;
    };
    let heave = paired_range(left, right, |l, r| (l + r) / 2.0);
    let roll = paired_range(left, right, |l, r| l - r);
    let longitudinal = force_range(measurement, ["lffx", "rffx"]) > ACD_FORCE_MOVES_N;
    let lateral = force_range(measurement, ["lffy", "rffy"]) > ACD_FORCE_MOVES_N;
    match (
        roll > ACD_MOVES_MM,
        heave > ACD_MOVES_MM,
        longitudinal,
        lateral,
    ) {
        (true, false, false, false) => Detection::Test("roll"),
        (false, true, false, false) => Detection::Test("vertical"),
        (false, false, true, false) => Detection::Test("longitudinal"),
        (false, false, false, true) => Detection::Test("lateral"),
        (false, false, false, false) => Detection::Test("static"),
        _ => Detection::Undetermined,
    }
}

const COMBINED: &str =
    "subtest not determinable from motion data (combined subtest file? unsupported)";

/// Warnings when motion data alone disagrees with, or cannot confirm, the declared subtest.
fn motion_warnings(motion: Detection, declared: &str) -> Vec<String> {
    match motion {
        Detection::Test(detected) if detected == declared => vec![],
        Detection::Test(detected) => {
            vec![format!(
                "motion data indicates '{detected}', but --subtest is '{declared}'"
            )]
        }
        Detection::Undetermined => vec![COMBINED.into()],
        Detection::Unavailable => vec!["no motion channels, subtest not verified".into()],
    }
}

/// Warnings for a declared KnC subtest checked against RES motion data.
pub fn res_warnings(result: &AdamsResult, declared: &str) -> Vec<String> {
    motion_warnings(res_subtest(result), declared)
}

/// Warnings for a declared KnC subtest checked against ACD `testtype` and motion data.
/// An empty list means everything agrees.
pub fn acd_warnings(measurement: &AcdMeasurement, declared: &str) -> Vec<String> {
    let motion = acd_motion_subtest(measurement);
    let Some(info) = measurement
        .test_info
        .iter()
        .find(|info| info.name == "testtype")
    else {
        return motion_warnings(motion, declared);
    };
    let Some((_, meta)) = acd_subtest(&info.value) else {
        return motion_warnings(motion, declared);
    };
    let tt = &info.value;
    let mismatch = meta != declared;
    let against = format!("but --subtest is '{declared}'");
    match motion {
        Detection::Test(detected) if detected == meta && mismatch => {
            vec![format!(
                "testtype '{tt}' and motion data indicate '{meta}', {against}"
            )]
        }
        Detection::Test(detected) if detected == meta => vec![],
        Detection::Test(detected) => vec![format!(
            "testtype '{tt}' indicates '{meta}', but motion data indicates '{detected}'"
        )],
        Detection::Unavailable if mismatch => vec![format!(
            "testtype '{tt}' indicates '{meta}', {against}; no motion channels to confirm"
        )],
        Detection::Unavailable => vec!["no motion channels, testtype not confirmed".into()],
        Detection::Undetermined => {
            let mut out = vec![format!("{COMBINED}; testtype not confirmed")];
            if mismatch {
                out.push(format!("testtype '{tt}' indicates '{meta}', {against}"));
            }
            out
        }
    }
}

/// Test an ACD `testtype` value names, if known.
fn acd_subtest(testtype: &str) -> Option<(&'static str, &'static str)> {
    match testtype {
        "vert" => Some(("knc", "vertical")),
        "roll" => Some(("knc", "roll")),
        "lat" => Some(("knc", "lateral")),
        "long" => Some(("knc", "longitudinal")),
        _ => None,
    }
}

/// Reject a declared identity the ACD `testtype` contradicts.
pub fn check_acd_identity(
    measurement: &AcdMeasurement,
    identity: &Identity,
    source: &str,
) -> Result<()> {
    let Some(info) = measurement
        .test_info
        .iter()
        .find(|info| info.name == "testtype")
    else {
        return Ok(());
    };
    let Some((test, _)) = acd_subtest(&info.value) else {
        return Ok(());
    };
    if test != identity.test {
        bail!(
            "--test {} but testtype '{}' in '{source}' indicates {test}",
            identity.test,
            info.value
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        COMBINED, Detection, Identity, acd_motion_subtest, acd_subtest, acd_warnings,
        check_acd_identity, res_subtest, res_test_warnings, res_warnings,
    };
    use crate::acd::{AcdChannel, AcdMeasurement, AcdTestInfo};
    use crate::model::{AdamsResult, Component, Entity};

    #[test]
    fn validates_test_and_subtest_pairs() {
        assert!(Identity::new("svc", None).is_ok());
        assert!(Identity::new("knc", Some("vertical")).is_ok());
        assert!(Identity::new("knc", Some("static")).is_ok());
        assert!(Identity::new("knc", Some("roll")).is_ok());
        assert!(Identity::new("knc", Some("lateral")).is_ok());
        assert!(Identity::new("knc", Some("longitudinal")).is_ok());
        for (test, subtest, message) in [
            ("driving", None, "unknown test 'driving'"),
            ("knc", None, "requires --subtest"),
            ("knc", Some("steering"), "unknown subtest 'steering'"),
            ("svc", Some("vertical"), "has no subtests"),
        ] {
            let error = Identity::new(test, subtest).unwrap_err().to_string();
            assert!(error.contains(message), "{error}");
        }
    }

    fn res_with_analysis_type(value: f64) -> AdamsResult {
        AdamsResult {
            entities: vec![Entity {
                name: "analysis_type".into(),
                source_path: None,
                object_id: None,
                components: vec![Component {
                    name: "Q".into(),
                    adams_id: 1,
                    units: None,
                    values: vec![value],
                }],
            }],
            sample_count: 1,
            values_per_sample: 1,
        }
    }

    #[test]
    fn res_test_warnings_cover_each_signal_state() {
        let knc = res_with_analysis_type(0.0);
        assert!(res_test_warnings(&knc, "knc").is_empty());
        assert_eq!(
            res_test_warnings(&knc, "svc"),
            ["analysis_type indicates 'knc', but --test is 'svc'"]
        );

        let mut split = res_with_analysis_type(1.0);
        split.entities.push(Entity {
            name: "wheel_travel".into(),
            source_path: None,
            object_id: None,
            components: vec![],
        });
        assert_eq!(
            res_test_warnings(&split, "knc"),
            ["analysis_type indicates 'svc', but --test is 'knc'"]
        );
        assert_eq!(
            res_test_warnings(&split, "svc"),
            ["wheel_travel entity indicates 'knc', but --test is 'svc'"]
        );

        let none = AdamsResult {
            entities: vec![],
            sample_count: 0,
            values_per_sample: 0,
        };
        assert_eq!(
            res_test_warnings(&none, "knc"),
            ["no analysis_type or wheel_travel, test not verified"]
        );
    }

    fn motion_entity(name: &str, values: Vec<f64>) -> Entity {
        Entity {
            name: name.into(),
            source_path: None,
            object_id: None,
            components: vec![Component {
                name: "Z".into(),
                adams_id: 1,
                units: None,
                values,
            }],
        }
    }

    fn res_with_motion(roll: Option<Vec<f64>>, vertical: Option<Vec<f64>>) -> AdamsResult {
        let mut entities = Vec::new();
        if let Some(values) = roll {
            entities.push(motion_entity("motion_roll", values));
        }
        if let Some(values) = vertical {
            entities.push(motion_entity("motion_vertical", values));
        }
        AdamsResult {
            entities,
            sample_count: 2,
            values_per_sample: 2,
        }
    }

    #[test]
    fn detects_test_from_motion() {
        let flat = || Some(vec![0.0, 0.0]);
        let cases = [
            (
                Some(vec![0.0, 0.14]),
                Some(vec![0.0, 2.3]),
                Detection::Test("roll"),
            ),
            (flat(), Some(vec![0.0, 100.0]), Detection::Test("vertical")),
            (flat(), Some(vec![0.0, 0.03]), Detection::Test("static")),
            (
                Some(vec![0.0, 0.14]),
                Some(vec![0.0, 100.0]),
                Detection::Undetermined,
            ),
            (None, None, Detection::Unavailable),
            (flat(), None, Detection::Unavailable),
        ];
        for (roll, vertical, expected) in cases {
            assert_eq!(res_subtest(&res_with_motion(roll, vertical)), expected);
        }
    }

    fn res_with_loads(loads: &[(&str, &str, f64)]) -> AdamsResult {
        let mut result = res_with_motion(Some(vec![0.0, 0.0]), Some(vec![0.0, 0.0]));
        for (entity, component, peak) in loads {
            result.entities.push(Entity {
                name: (*entity).into(),
                source_path: None,
                object_id: None,
                components: vec![Component {
                    name: (*component).into(),
                    adams_id: 1,
                    units: None,
                    values: vec![0.0, *peak],
                }],
            });
        }
        result
    }

    #[test]
    fn detects_force_driven_tests_from_wheel_loads() {
        let cases = [
            (
                vec![
                    ("gforce_tire_lf", "FX", 3000.0),
                    ("gforce_tire_rf", "FX", 3000.0),
                ],
                Detection::Test("longitudinal"),
            ),
            (
                vec![
                    ("gforce_tire_lf", "FY", 3000.0),
                    ("gforce_tire_rf", "FY", -3000.0),
                ],
                Detection::Test("lateral"),
            ),
            (
                vec![
                    ("gforce_tire_lf", "FY", 3000.0),
                    ("gforce_tire_lf", "FX", 3000.0),
                ],
                Detection::Undetermined,
            ),
            (
                vec![
                    ("gforce_tire_lf", "FY", 1500.0),
                    ("gforce_tire_lf", "TZ", 45000.0),
                ],
                Detection::Test("lateral"),
            ),
            (vec![("gforce_tire_lf", "TZ", 5e5)], Detection::Undetermined),
            (
                vec![("steering_wheel_input", "steering_wheel_input", 12.5)],
                Detection::Undetermined,
            ),
            (
                vec![("gforce_tire_lf", "FY", 0.0)],
                Detection::Test("static"),
            ),
            (vec![], Detection::Test("static")),
        ];
        for (loads, expected) in cases {
            assert_eq!(res_subtest(&res_with_loads(&loads)), expected, "{loads:?}");
        }
    }

    #[test]
    fn wheel_load_with_rig_motion_is_undetermined() {
        let mut result = res_with_loads(&[("gforce_tire_lf", "FY", 3000.0)]);
        result.entities[1].components[0].values = vec![0.0, 100.0];
        assert_eq!(res_subtest(&result), Detection::Undetermined);
    }

    #[test]
    fn detects_acd_test_from_motion_and_metadata() {
        let example = |name: &str| {
            let path = format!(
                "{}/../../examples/input/raw-measurement/{name}",
                env!("CARGO_MANIFEST_DIR")
            );
            crate::acd::parse_file(path).unwrap()
        };
        for (file, subtest) in [
            ("example-measurement-knc-roll.acd", "roll"),
            ("example-measurement-knc-vertical.acd", "vertical"),
        ] {
            let measurement = example(file);
            assert_eq!(acd_motion_subtest(&measurement), Detection::Test(subtest));
            assert!(acd_warnings(&measurement, subtest).is_empty());
        }

        let roll = example("example-measurement-knc-roll.acd");
        assert_eq!(
            acd_warnings(&roll, "vertical"),
            ["testtype 'roll' and motion data indicate 'roll', but --subtest is 'vertical'"]
        );

        let mut disagree = roll.clone();
        for info in &mut disagree.test_info {
            if info.name == "testtype" {
                info.value = "vert".into();
            }
        }
        assert_eq!(
            acd_warnings(&disagree, "roll"),
            ["testtype 'vert' indicates 'vertical', but motion data indicates 'roll'"]
        );

        let mut no_motion = roll;
        no_motion.channels.retain(|c| c.name != "lfwcz");
        assert_eq!(
            acd_warnings(&no_motion, "roll"),
            ["no motion channels, testtype not confirmed"]
        );

        no_motion.test_info.retain(|i| i.name != "testtype");
        assert_eq!(
            acd_warnings(&no_motion, "roll"),
            ["no motion channels, subtest not verified"]
        );
    }

    fn acd_with_forces(forces: &[(&str, f64)]) -> AcdMeasurement {
        let mut measurement = acd_with_testtype("lat");
        measurement.channels = ["lfwcz", "rfwcz"]
            .iter()
            .map(|name| (*name, 0.0))
            .chain(forces.iter().copied())
            .map(|(name, peak)| AcdChannel {
                name: name.into(),
                description: String::new(),
                unit: String::new(),
                offset: 0.0,
                column: 0,
                values: vec![-peak, peak],
            })
            .collect();
        measurement
    }

    #[test]
    fn detects_acd_compliance_from_wheel_forces() {
        let lateral = acd_with_forces(&[("lffy", 3000.0), ("rffy", 3000.0)]);
        assert_eq!(acd_motion_subtest(&lateral), Detection::Test("lateral"));
        assert!(acd_warnings(&lateral, "lateral").is_empty());
        let longitudinal = acd_with_forces(&[("lffx", 3000.0), ("rffx", 3000.0)]);
        assert_eq!(
            acd_motion_subtest(&longitudinal),
            Detection::Test("longitudinal")
        );
        let both = acd_with_forces(&[("lffx", 3000.0), ("lffy", 3000.0)]);
        assert_eq!(acd_motion_subtest(&both), Detection::Undetermined);
        let rest = acd_with_forces(&[("lffy", 150.0)]);
        assert_eq!(acd_motion_subtest(&rest), Detection::Test("static"));
        assert_eq!(acd_subtest("lat"), Some(("knc", "lateral")));
        assert_eq!(acd_subtest("long"), Some(("knc", "longitudinal")));
    }

    #[test]
    fn res_warnings_cover_each_detection() {
        let roll = res_with_motion(Some(vec![0.0, 0.14]), Some(vec![0.0, 2.3]));
        assert!(res_warnings(&roll, "roll").is_empty());
        assert_eq!(
            res_warnings(&roll, "vertical"),
            ["motion data indicates 'roll', but --subtest is 'vertical'"]
        );
        let both = res_with_motion(Some(vec![0.0, 0.14]), Some(vec![0.0, 100.0]));
        assert_eq!(res_warnings(&both, "roll"), [COMBINED]);
        let none = res_with_motion(None, None);
        assert_eq!(
            res_warnings(&none, "roll"),
            ["no motion channels, subtest not verified"]
        );
    }

    fn acd_with_testtype(value: &str) -> AcdMeasurement {
        AcdMeasurement {
            encoding: "ASCII3".into(),
            dialect: "ASCII3".into(),
            channels: Vec::new(),
            test_info: vec![AcdTestInfo {
                name: "testtype".into(),
                description: String::new(),
                data_type: "string".into(),
                unit: String::new(),
                value: value.into(),
            }],
        }
    }

    #[test]
    fn acd_identity_passes_when_testtype_agrees() {
        let identity = Identity::new("knc", Some("vertical")).unwrap();
        check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd").unwrap();
    }

    #[test]
    fn acd_identity_maps_roll_testtype() {
        let identity = Identity::new("knc", Some("roll")).unwrap();
        check_acd_identity(&acd_with_testtype("roll"), &identity, "source.acd").unwrap();
        let identity = Identity::new("knc", Some("vertical")).unwrap();
        check_acd_identity(&acd_with_testtype("roll"), &identity, "source.acd").unwrap();
    }

    #[test]
    fn acd_identity_rejects_test_the_testtype_contradicts() {
        let identity = Identity::new("svc", None).unwrap();
        let error = check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--test svc"), "{error}");
        assert!(error.contains("indicates knc"), "{error}");
    }

    #[test]
    fn acd_identity_leaves_test_contradiction_to_warnings() {
        let identity = Identity::new("knc", Some("static")).unwrap();
        check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd").unwrap();
        assert_eq!(
            acd_warnings(&acd_with_testtype("vert"), "static"),
            [
                "testtype 'vert' indicates 'vertical', but --subtest is 'static'; no motion channels to confirm"
            ]
        );
        assert_eq!(
            acd_warnings(&acd_with_testtype("vert"), "vertical"),
            ["no motion channels, testtype not confirmed"]
        );
    }
}
