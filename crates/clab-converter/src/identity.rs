//! Domain and test a converted file belongs to.
//!
//! The caller declares both. The data cross-checks the declaration before
//! anything is written: a declared domain or test the data contradicts is
//! a hard error, not a warning, so a canonical HDF5 file is never written
//! under the wrong identity.

use anyhow::{Result, bail};

use crate::acd::AcdMeasurement;
use crate::model::AdamsResult;

/// Known domains and their tests. Add new domains and tests here.
pub const DOMAINS: &[(&str, &[&str])] = &[("svc", &[]), ("knc", &["vertical", "roll", "static"])];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub domain: String,
    pub test: Option<String>,
}

impl Identity {
    /// Validate a declared domain and test against the known domains.
    pub fn new(domain: &str, test: Option<&str>) -> Result<Self> {
        let known = |names: &[&str]| names.join(", ");
        let Some((_, tests)) = DOMAINS.iter().find(|(name, _)| *name == domain) else {
            let domains: Vec<_> = DOMAINS.iter().map(|(name, _)| *name).collect();
            bail!(
                "unknown domain '{domain}'; expected one of: {}",
                known(&domains)
            );
        };
        match (tests.is_empty(), test) {
            (true, Some(test)) => {
                bail!("domain '{domain}' has no tests; remove --test {test}")
            }
            (false, None) => bail!(
                "domain '{domain}' requires --test; expected one of: {}",
                known(tests)
            ),
            (false, Some(test)) if !tests.contains(&test) => bail!(
                "unknown test '{test}' for domain '{domain}'; expected one of: {}",
                known(tests)
            ),
            _ => Ok(Self {
                domain: domain.into(),
                test: test.map(Into::into),
            }),
        }
    }
}

/// Domain an Adams result indicates through its `analysis_type` variable:
/// 0 is a KnC rig analysis, 1 an SVC analysis. The last sample is used,
/// because SVC results start at 0.
pub fn res_domain(result: &AdamsResult) -> Option<&'static str> {
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
fn res_structural_domain(result: &AdamsResult) -> Option<&'static str> {
    result
        .entities
        .iter()
        .any(|entity| entity.name.contains("wheel_travel"))
        .then_some("knc")
}

/// Warnings when the `analysis_type` value or a `wheel_travel` entity
/// contradicts the declared domain, or when neither signal exists.
pub fn res_domain_warnings(result: &AdamsResult, declared: &str) -> Vec<String> {
    let signals = [
        ("analysis_type", res_domain(result)),
        ("wheel_travel entity", res_structural_domain(result)),
    ];
    if signals.iter().all(|(_, found)| found.is_none()) {
        return vec!["no analysis_type or wheel_travel, domain not verified".into()];
    }
    signals
        .into_iter()
        .filter_map(|(signal, found)| {
            found
                .filter(|found| *found != declared)
                .map(|found| format!("{signal} indicates '{found}', but --domain is '{declared}'"))
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

/// Largest peak-to-peak range over all components of the named entity.
fn motion_range(result: &AdamsResult, entity_name: &str) -> Option<f64> {
    let mut found = false;
    let mut range = 0.0f64;
    for entity in result.entities.iter().filter(|e| e.name == entity_name) {
        for component in &entity.components {
            found = true;
            let (min, max) = component
                .values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                });
            if max >= min {
                range = range.max(max - min);
            }
        }
    }
    found.then_some(range)
}

/// Detect the KnC test from which rig motion moves.
pub fn res_test(result: &AdamsResult) -> Detection {
    let (Some(roll), Some(vertical)) = (
        motion_range(result, "motion_roll"),
        motion_range(result, "motion_vertical"),
    ) else {
        return Detection::Unavailable;
    };
    match (roll > ROLL_MOVES_RAD, vertical > VERTICAL_MOVES_MM) {
        (true, false) => Detection::Test("roll"),
        (false, true) => Detection::Test("vertical"),
        (false, false) => Detection::Test("static"),
        (true, true) => Detection::Undetermined,
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

/// Detect the KnC test from front wheel-centre Z: vertical moves both
/// wheels together (heave), roll moves them in opposition.
pub fn acd_motion_test(measurement: &AcdMeasurement) -> Detection {
    let (Some(left), Some(right)) = (
        acd_channel(measurement, "lfwcz"),
        acd_channel(measurement, "rfwcz"),
    ) else {
        return Detection::Unavailable;
    };
    let heave = paired_range(left, right, |l, r| (l + r) / 2.0);
    let roll = paired_range(left, right, |l, r| l - r);
    match (roll > ACD_MOVES_MM, heave > ACD_MOVES_MM) {
        (true, false) => Detection::Test("roll"),
        (false, true) => Detection::Test("vertical"),
        (false, false) => Detection::Test("static"),
        (true, true) => Detection::Undetermined,
    }
}

const COMBINED: &str = "test not determinable from motion data (combined test file? unsupported)";

/// Warnings when motion data alone disagrees with, or cannot confirm, the declared test.
fn motion_warnings(motion: Detection, declared: &str) -> Vec<String> {
    match motion {
        Detection::Test(detected) if detected == declared => vec![],
        Detection::Test(detected) => {
            vec![format!(
                "motion data indicates '{detected}', but --test is '{declared}'"
            )]
        }
        Detection::Undetermined => vec![COMBINED.into()],
        Detection::Unavailable => vec!["no motion channels, test not verified".into()],
    }
}

/// Warnings for a declared KnC test checked against RES motion data.
pub fn res_warnings(result: &AdamsResult, declared: &str) -> Vec<String> {
    motion_warnings(res_test(result), declared)
}

/// Warnings for a declared KnC test checked against ACD `testtype` and motion data.
/// An empty list means everything agrees.
pub fn acd_warnings(measurement: &AcdMeasurement, declared: &str) -> Vec<String> {
    let motion = acd_motion_test(measurement);
    let Some(info) = measurement
        .test_info
        .iter()
        .find(|info| info.name == "testtype")
    else {
        return motion_warnings(motion, declared);
    };
    let Some((_, meta)) = acd_test(&info.value) else {
        return motion_warnings(motion, declared);
    };
    let tt = &info.value;
    let mismatch = meta != declared;
    let against = format!("but --test is '{declared}'");
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
fn acd_test(testtype: &str) -> Option<(&'static str, &'static str)> {
    match testtype {
        "vert" => Some(("knc", "vertical")),
        "roll" => Some(("knc", "roll")),
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
    let Some((domain, _)) = acd_test(&info.value) else {
        return Ok(());
    };
    if domain != identity.domain {
        bail!(
            "--domain {} but testtype '{}' in '{source}' indicates {domain}",
            identity.domain,
            info.value
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        COMBINED, Detection, Identity, acd_motion_test, acd_warnings, check_acd_identity,
        res_domain_warnings, res_test, res_warnings,
    };
    use crate::acd::{AcdMeasurement, AcdTestInfo};
    use crate::model::{AdamsResult, Component, Entity};

    #[test]
    fn validates_domain_and_test_pairs() {
        assert!(Identity::new("svc", None).is_ok());
        assert!(Identity::new("knc", Some("vertical")).is_ok());
        assert!(Identity::new("knc", Some("static")).is_ok());
        assert!(Identity::new("knc", Some("roll")).is_ok());
        for (domain, test, message) in [
            ("driving", None, "unknown domain 'driving'"),
            ("knc", None, "requires --test"),
            ("knc", Some("lateral"), "unknown test 'lateral'"),
            ("svc", Some("vertical"), "has no tests"),
        ] {
            let error = Identity::new(domain, test).unwrap_err().to_string();
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
    fn res_domain_warnings_cover_each_signal_state() {
        let knc = res_with_analysis_type(0.0);
        assert!(res_domain_warnings(&knc, "knc").is_empty());
        assert_eq!(
            res_domain_warnings(&knc, "svc"),
            ["analysis_type indicates 'knc', but --domain is 'svc'"]
        );

        let mut split = res_with_analysis_type(1.0);
        split.entities.push(Entity {
            name: "wheel_travel".into(),
            source_path: None,
            object_id: None,
            components: vec![],
        });
        assert_eq!(
            res_domain_warnings(&split, "knc"),
            ["analysis_type indicates 'svc', but --domain is 'knc'"]
        );
        assert_eq!(
            res_domain_warnings(&split, "svc"),
            ["wheel_travel entity indicates 'knc', but --domain is 'svc'"]
        );

        let none = AdamsResult {
            entities: vec![],
            sample_count: 0,
            values_per_sample: 0,
        };
        assert_eq!(
            res_domain_warnings(&none, "knc"),
            ["no analysis_type or wheel_travel, domain not verified"]
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
            assert_eq!(res_test(&res_with_motion(roll, vertical)), expected);
        }
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
        for (file, test) in [
            ("example-measurement-knc-roll.acd", "roll"),
            ("example-measurement-knc-vertical.acd", "vertical"),
        ] {
            let measurement = example(file);
            assert_eq!(acd_motion_test(&measurement), Detection::Test(test));
            assert!(acd_warnings(&measurement, test).is_empty());
        }

        let roll = example("example-measurement-knc-roll.acd");
        assert_eq!(
            acd_warnings(&roll, "vertical"),
            ["testtype 'roll' and motion data indicate 'roll', but --test is 'vertical'"]
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
            ["no motion channels, test not verified"]
        );
    }

    #[test]
    fn res_warnings_cover_each_detection() {
        let roll = res_with_motion(Some(vec![0.0, 0.14]), Some(vec![0.0, 2.3]));
        assert!(res_warnings(&roll, "roll").is_empty());
        assert_eq!(
            res_warnings(&roll, "vertical"),
            ["motion data indicates 'roll', but --test is 'vertical'"]
        );
        let both = res_with_motion(Some(vec![0.0, 0.14]), Some(vec![0.0, 100.0]));
        assert_eq!(res_warnings(&both, "roll"), [COMBINED]);
        let none = res_with_motion(None, None);
        assert_eq!(
            res_warnings(&none, "roll"),
            ["no motion channels, test not verified"]
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
    fn acd_identity_rejects_domain_the_testtype_contradicts() {
        let identity = Identity::new("svc", None).unwrap();
        let error = check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--domain svc"), "{error}");
        assert!(error.contains("indicates knc"), "{error}");
    }

    #[test]
    fn acd_identity_leaves_test_contradiction_to_warnings() {
        let identity = Identity::new("knc", Some("static")).unwrap();
        check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd").unwrap();
        assert_eq!(
            acd_warnings(&acd_with_testtype("vert"), "static"),
            [
                "testtype 'vert' indicates 'vertical', but --test is 'static'; no motion channels to confirm"
            ]
        );
        assert_eq!(
            acd_warnings(&acd_with_testtype("vert"), "vertical"),
            ["no motion channels, testtype not confirmed"]
        );
    }
}
