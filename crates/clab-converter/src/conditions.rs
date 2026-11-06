//! Test setup declared at conversion: setup code, lateral phase and offset.
//!
//! The caller declares every condition. Whatever the data can show is
//! compared with the declaration; a disagreement is a warning.

use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use hdf5::File;

use crate::acd::AcdMeasurement;
use crate::hdf5_writer::write_string_attribute;
use crate::model::AdamsResult;

/// Correlation of left and right front Fy above which the phase is clear.
const PHASE_CORRELATION: f64 = 0.9;
/// Tolerance (mm) between the declared offset and Tz ÷ Fy.
/// Measured: real files deviate at most 2 mm.
const OFFSET_TOLERANCE_MM: f64 = 5.0;
/// Brake torque (N·mm) above which a RES run brakes.
/// Measured: braking 4.6e5, all other runs at most 1.5e4.
const BRAKE_TORQUE_NMM: f64 = 1e5;

/// Setup code `b<0|1>e<0|1>f<0|1>r<0|1>`: brakes applied, steering engine on,
/// front and rear anti-roll bar connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub brakes: bool,
    pub engine: bool,
    pub front_arb: bool,
    pub rear_arb: bool,
}

impl FromStr for Setup {
    type Err = String;

    fn from_str(code: &str) -> Result<Self, String> {
        let invalid =
            || format!("invalid setup '{code}'; expected b<0|1>e<0|1>f<0|1>r<0|1>, e.g. b1e0f1r1");
        let chars: Vec<char> = code.to_ascii_lowercase().chars().collect();
        let [b, bv, e, ev, f, fv, r, rv] = chars[..] else {
            return Err(invalid());
        };
        let bit = |c: char| match c {
            '0' => Some(false),
            '1' => Some(true),
            _ => None,
        };
        match ([b, e, f, r], [bit(bv), bit(ev), bit(fv), bit(rv)]) {
            (
                ['b', 'e', 'f', 'r'],
                [Some(brakes), Some(engine), Some(front_arb), Some(rear_arb)],
            ) => Ok(Self {
                brakes,
                engine,
                front_arb,
                rear_arb,
            }),
            _ => Err(invalid()),
        }
    }
}

impl fmt::Display for Setup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bit = |on: bool| u8::from(on);
        write!(
            f,
            "B{}E{}F{}R{}",
            bit(self.brakes),
            bit(self.engine),
            bit(self.front_arb),
            bit(self.rear_arb)
        )
    }
}

/// Lateral force phase of the left and right wheels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    In,
    Anti,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::In => "in",
            Self::Anti => "anti",
        }
    }
}

impl FromStr for Phase {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "in" => Ok(Self::In),
            "anti" => Ok(Self::Anti),
            _ => Err(format!("invalid phase '{value}'; expected in or anti")),
        }
    }
}

/// Conditions of one converted KnC file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    pub setup: Setup,
    /// Lateral tests only.
    pub phase: Option<Phase>,
    /// Pneumatic trail offset in mm, lateral tests only.
    pub offset_mm: Option<f64>,
}

impl Conditions {
    /// Validate the declared flags against the domain and test.
    pub fn declare(
        domain: &str,
        test: Option<&str>,
        setup: Option<Setup>,
        phase: Option<Phase>,
        offset_mm: Option<f64>,
    ) -> Result<Option<Self>> {
        let lateral = test == Some("lateral");
        match (domain, setup) {
            ("knc", None) => anyhow::bail!("domain 'knc' requires --setup, e.g. b1e0f1r1"),
            ("knc", Some(setup)) => {
                if lateral && (phase.is_none() || offset_mm.is_none()) {
                    anyhow::bail!("test 'lateral' requires --phase and --offset");
                }
                if !lateral && (phase.is_some() || offset_mm.is_some()) {
                    anyhow::bail!("--phase and --offset only apply to test 'lateral'");
                }
                if offset_mm.is_some_and(|mm| !mm.is_finite() || mm < 0.0) {
                    anyhow::bail!("--offset must be a non-negative number of mm");
                }
                Ok(Some(Self {
                    setup,
                    phase,
                    offset_mm,
                }))
            }
            (_, None) if phase.is_none() && offset_mm.is_none() => Ok(None),
            _ => anyhow::bail!(
                "domain '{domain}' takes no --setup, --phase or --offset; they only apply to knc"
            ),
        }
    }

    pub fn write(&self, file: &File) -> Result<()> {
        write_string_attribute(file, "setup", &self.setup.to_string())?;
        if let Some(phase) = self.phase {
            write_string_attribute(file, "phase", phase.as_str())?;
        }
        if let Some(offset) = self.offset_mm {
            file.new_attr::<f64>()
                .create("offset_mm")?
                .write_scalar(&offset)?;
        }
        Ok(())
    }
}

/// One declared condition and what the data says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    name: &'static str,
    value: String,
    confirmed_by: Vec<&'static str>,
    contradictions: Vec<String>,
}

impl Finding {
    fn new(name: &'static str, value: impl fmt::Display) -> Self {
        Self {
            name,
            value: value.to_string(),
            confirmed_by: Vec::new(),
            contradictions: Vec::new(),
        }
    }

    /// Record what `source` shows; `None` means it shows nothing.
    fn compare(&mut self, source: &'static str, detected: Option<String>) {
        match detected {
            Some(found) if found == self.value => self.confirmed_by.push(source),
            Some(found) => self.contradictions.push(format!(
                "{}={} declared, but {source} indicates {found}",
                self.name, self.value
            )),
            None => {}
        }
    }

    pub fn warnings(&self) -> &[String] {
        &self.contradictions
    }
}

impl fmt::Display for Finding {
    /// `B=1 [header, brake torque]`, `[declared]` when nothing confirms it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut sources = self.confirmed_by.clone();
        if sources.is_empty() {
            sources.push("declared");
        }
        if !self.contradictions.is_empty() {
            sources.push("MISMATCH");
        }
        write!(f, "{}={} [{}]", self.name, self.value, sources.join(", "))
    }
}

fn bit(on: bool) -> String {
    u8::from(on).to_string()
}

fn header<'a>(measurement: &'a AcdMeasurement, name: &str) -> Option<&'a str> {
    measurement
        .test_info
        .iter()
        .find(|info| info.name == name)
        .map(|info| info.value.trim())
}

fn header_flag(measurement: &AcdMeasurement, name: &str) -> Option<String> {
    match header(measurement, name)? {
        "1" => Some("1".into()),
        "0" => Some("0".into()),
        _ => None,
    }
}

/// Pearson correlation of the finite sample pairs.
fn correlation(left: &[f64], right: &[f64]) -> Option<f64> {
    let pairs: Vec<_> = left
        .iter()
        .zip(right)
        .filter(|(l, r)| l.is_finite() && r.is_finite())
        .collect();
    let n = pairs.len() as f64;
    let (mean_l, mean_r) = (
        pairs.iter().map(|(l, _)| **l).sum::<f64>() / n,
        pairs.iter().map(|(_, r)| **r).sum::<f64>() / n,
    );
    let (mut cov, mut var_l, mut var_r) = (0.0, 0.0, 0.0);
    for (l, r) in pairs {
        cov += (l - mean_l) * (r - mean_r);
        var_l += (l - mean_l).powi(2);
        var_r += (r - mean_r).powi(2);
    }
    let denominator = (var_l * var_r).sqrt();
    (denominator > 0.0).then(|| cov / denominator)
}

fn phase_from_fy(left: &[f64], right: &[f64]) -> Option<Phase> {
    match correlation(left, right)? {
        r if r >= PHASE_CORRELATION => Some(Phase::In),
        r if r <= -PHASE_CORRELATION => Some(Phase::Anti),
        _ => None,
    }
}

/// Mean over the wheels of |Tz ÷ Fy| (least squares) in mm; `to_mm` converts the Tz unit.
fn offset_from_tz(wheels: &[(&[f64], &[f64])], to_mm: f64) -> Option<f64> {
    let slopes: Vec<f64> = wheels
        .iter()
        .filter_map(|(tz, fy)| {
            let (num, den) = tz
                .iter()
                .zip(*fy)
                .filter(|(t, f)| t.is_finite() && f.is_finite())
                .fold((0.0, 0.0), |(n, d), (t, f)| (n + t * f, d + f * f));
            (den > 0.0).then(|| (num / den).abs() * to_mm)
        })
        .collect();
    (!slopes.is_empty()).then(|| slopes.iter().sum::<f64>() / slopes.len() as f64)
}

fn offset_finding(declared: f64, header_mm: Option<f64>, signal_mm: Option<f64>) -> Finding {
    let mut finding = Finding::new("offset_mm", declared);
    finding.compare("header", header_mm.map(|mm| mm.to_string()));
    finding.compare(
        "Tz/Fy",
        signal_mm.map(|mm| {
            if (mm - declared).abs() <= OFFSET_TOLERANCE_MM {
                declared.to_string()
            } else {
                format!("{mm:.0}")
            }
        }),
    );
    finding
}

fn phase_finding(declared: Phase, header: Option<Phase>, signal: Option<Phase>) -> Finding {
    let mut finding = Finding::new("phase", declared.as_str());
    finding.compare("header", header.map(|p| p.as_str().into()));
    finding.compare("Fy", signal.map(|p| p.as_str().into()));
    finding
}

/// Findings for an ACD measurement: setup flags and wheel mode from the header,
/// lateral phase and offset from the header and the front wheel forces.
pub fn acd_findings(measurement: &AcdMeasurement, test: &str, c: &Conditions) -> Vec<Finding> {
    let channel = |name: &str| {
        measurement
            .channels
            .iter()
            .find(|channel| channel.name == name)
    };
    let mut brakes = Finding::new("B", bit(c.setup.brakes));
    brakes.compare("header", header_flag(measurement, "brakeson"));
    if test == "longitudinal" {
        // Strapped wheels are the unbraked acceleration run.
        brakes.compare(
            "wheel mode",
            header(measurement, "fwheelmode").map(|mode| bit(mode != "Straps")),
        );
    }
    let mut engine = Finding::new("E", bit(c.setup.engine));
    engine.compare("header", header_flag(measurement, "engineon"));
    let mut front = Finding::new("F", bit(c.setup.front_arb));
    front.compare("header", header_flag(measurement, "farbon"));
    let mut rear = Finding::new("R", bit(c.setup.rear_arb));
    rear.compare("header", header_flag(measurement, "rarbon"));
    let mut findings = vec![brakes, engine, front, rear];

    if let (Some(phase), Some(offset)) = (c.phase, c.offset_mm) {
        let fy = |name| channel(name).map(|c| c.values.as_slice());
        let signal_phase = fy("lffy")
            .zip(fy("rffy"))
            .and_then(|(l, r)| phase_from_fy(l, r));
        let header_phase = header(measurement, "subtest1").and_then(|value| match value {
            "aid" => Some(Phase::In),
            "opp" => Some(Phase::Anti),
            _ => None,
        });
        findings.push(phase_finding(phase, header_phase, signal_phase));

        let to_mm = channel("lftz")
            .is_some_and(|c| c.unit == "Nm")
            .then_some(1000.0);
        let wheels: Vec<_> = [("lftz", "lffy"), ("rftz", "rffy")]
            .iter()
            .filter_map(|(tz, fy_name)| Some((channel(tz)?.values.as_slice(), fy(fy_name)?)))
            .collect();
        let signal_offset = to_mm.and_then(|to_mm| offset_from_tz(&wheels, to_mm));
        let header_offset = header(measurement, "subtest2").and_then(|v| v.parse().ok());
        findings.push(offset_finding(offset, header_offset, signal_offset));
    }
    findings
}

fn res_component<'a>(result: &'a AdamsResult, entity: &str, component: &str) -> Option<&'a [f64]> {
    result
        .entities
        .iter()
        .filter(|e| e.name == entity)
        .flat_map(|e| &e.components)
        .find(|c| c.name == component)
        .map(|c| c.values.as_slice())
}

/// Findings for a RES result: only what the signals show is compared. Brakes
/// show in the brake torque of a longitudinal run; phase and offset in the front wheel forces.
/// Everything else stays declared.
pub fn res_findings(result: &AdamsResult, test: &str, c: &Conditions) -> Vec<Finding> {
    let mut brakes = Finding::new("B", bit(c.setup.brakes));
    if test == "longitudinal" {
        let peak = result
            .entities
            .iter()
            .filter(|e| e.name.starts_with("brake_torque_"))
            .flat_map(|e| &e.components)
            .flat_map(|c| &c.values)
            .map(|v| v.abs())
            .reduce(f64::max);
        brakes.compare("brake torque", peak.map(|p| bit(p > BRAKE_TORQUE_NMM)));
    }
    let mut findings = vec![
        brakes,
        Finding::new("E", bit(c.setup.engine)),
        Finding::new("F", bit(c.setup.front_arb)),
        Finding::new("R", bit(c.setup.rear_arb)),
    ];

    if let (Some(phase), Some(offset)) = (c.phase, c.offset_mm) {
        let force = |wheel: &str, component| {
            res_component(result, &format!("gforce_tire_{wheel}"), component)
        };
        let signal_phase = force("lf", "FY")
            .zip(force("rf", "FY"))
            .and_then(|(l, r)| phase_from_fy(l, r));
        findings.push(phase_finding(phase, None, signal_phase));

        let wheels: Vec<_> = ["lf", "rf"]
            .iter()
            .filter_map(|wheel| Some((force(wheel, "TZ")?, force(wheel, "FY")?)))
            .collect();
        findings.push(offset_finding(offset, None, offset_from_tz(&wheels, 1.0)));
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acd::{AcdChannel, AcdTestInfo};
    use crate::model::{Component, Entity};

    #[test]
    fn parses_setup_codes() {
        let setup: Setup = "B1e0F1r1".parse().unwrap();
        assert_eq!(
            setup,
            Setup {
                brakes: true,
                engine: false,
                front_arb: true,
                rear_arb: true
            }
        );
        assert_eq!(setup.to_string(), "B1E0F1R1");
        for bad in ["", "b1e0f1", "b1e0f1r2", "e1b0f1r1", "b1e0f1r1x"] {
            assert!(bad.parse::<Setup>().is_err(), "{bad}");
        }
    }

    #[test]
    fn declares_conditions_per_test() {
        let setup = Some("b1e0f1r1".parse().unwrap());
        let ok = |test, phase, offset| Conditions::declare("knc", Some(test), setup, phase, offset);
        assert!(ok("roll", None, None).unwrap().is_some());
        assert!(ok("lateral", Some(Phase::In), Some(30.0)).is_ok());
        assert!(ok("lateral", Some(Phase::In), None).is_err());
        assert!(ok("roll", Some(Phase::In), None).is_err());
        assert!(ok("lateral", Some(Phase::In), Some(-1.0)).is_err());
        assert!(Conditions::declare("knc", Some("roll"), None, None, None).is_err());
        assert!(
            Conditions::declare("svc", None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(Conditions::declare("svc", None, setup, None, None).is_err());
    }

    fn info(name: &str, value: &str) -> AcdTestInfo {
        AcdTestInfo {
            name: name.into(),
            description: String::new(),
            data_type: String::new(),
            unit: String::new(),
            value: value.into(),
        }
    }

    fn channel(name: &str, unit: &str, values: Vec<f64>) -> AcdChannel {
        AcdChannel {
            name: name.into(),
            description: String::new(),
            unit: unit.into(),
            offset: 0.0,
            column: 0,
            values,
        }
    }

    fn lateral_conditions(phase: Phase, offset: f64) -> Conditions {
        Conditions {
            setup: "b1e0f1r1".parse().unwrap(),
            phase: Some(phase),
            offset_mm: Some(offset),
        }
    }

    fn acd_lateral() -> AcdMeasurement {
        let fy = vec![-3000.0, 3000.0, -1000.0];
        let tz = fy.iter().map(|f| f * 0.03).collect();
        AcdMeasurement {
            encoding: String::new(),
            dialect: String::new(),
            channels: vec![
                channel("lffy", "N", fy.clone()),
                channel("rffy", "N", fy),
                channel("lftz", "Nm", tz),
            ],
            test_info: vec![
                info("brakeson", "1"),
                info("engineon", "0"),
                info("farbon", "1"),
                info("rarbon", "NaN"),
                info("subtest1", "aid"),
                info("subtest2", "30"),
            ],
        }
    }

    fn warnings(findings: &[Finding]) -> Vec<String> {
        findings
            .iter()
            .flat_map(|f| f.warnings().to_vec())
            .collect()
    }

    #[test]
    fn acd_findings_confirm_a_matching_declaration() {
        let findings = acd_findings(
            &acd_lateral(),
            "lateral",
            &lateral_conditions(Phase::In, 30.0),
        );
        assert!(warnings(&findings).is_empty());
        let text: Vec<_> = findings.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                "B=1 [header]",
                "E=0 [header]",
                "F=1 [header]",
                "R=1 [declared]",
                "phase=in [header, Fy]",
                "offset_mm=30 [header, Tz/Fy]"
            ]
        );
    }

    #[test]
    fn acd_findings_warn_on_each_disagreement() {
        let mut conditions = lateral_conditions(Phase::Anti, 0.0);
        conditions.setup.brakes = false;
        let found = warnings(&acd_findings(&acd_lateral(), "lateral", &conditions));
        assert_eq!(
            found,
            [
                "B=0 declared, but header indicates 1",
                "phase=anti declared, but header indicates in",
                "phase=anti declared, but Fy indicates in",
                "offset_mm=0 declared, but header indicates 30",
                "offset_mm=0 declared, but Tz/Fy indicates 30"
            ]
        );
    }

    #[test]
    fn acd_wheel_mode_checks_longitudinal_brakes() {
        let mut m = acd_lateral();
        m.test_info.push(info("fwheelmode", "Straps"));
        let c = lateral_conditions(Phase::In, 30.0);
        assert_eq!(warnings(&acd_findings(&m, "longitudinal", &c)).len(), 1);
        assert_eq!(warnings(&acd_findings(&m, "lateral", &c)).len(), 0);
    }

    fn res_with(entities: &[(&str, &str, Vec<f64>)]) -> AdamsResult {
        AdamsResult {
            entities: entities
                .iter()
                .map(|(entity, component, values)| Entity {
                    name: (*entity).into(),
                    source_path: None,
                    object_id: None,
                    components: vec![Component {
                        name: (*component).into(),
                        adams_id: 1,
                        units: None,
                        values: values.clone(),
                    }],
                })
                .collect(),
            sample_count: 0,
            values_per_sample: 0,
        }
    }

    #[test]
    fn res_findings_compare_phase_offset_and_brake_torque() {
        let fy = vec![-1500.0, 1500.0];
        let tz: Vec<f64> = fy.iter().map(|f| f * 30.0).collect();
        let lateral = res_with(&[
            ("gforce_tire_lf", "FY", fy.clone()),
            ("gforce_tire_rf", "FY", fy.iter().map(|f| -f).collect()),
            ("gforce_tire_lf", "TZ", tz),
        ]);
        let findings = res_findings(&lateral, "lateral", &lateral_conditions(Phase::Anti, 30.0));
        assert!(warnings(&findings).is_empty());
        assert_eq!(findings[4].to_string(), "phase=anti [Fy]");
        assert_eq!(findings[5].to_string(), "offset_mm=30 [Tz/Fy]");
        let wrong = res_findings(&lateral, "lateral", &lateral_conditions(Phase::In, 0.0));
        assert_eq!(warnings(&wrong).len(), 2);

        let braking = res_with(&[("brake_torque_lf", "Q", vec![0.0, 4.6e5])]);
        let mut c = lateral_conditions(Phase::In, 0.0);
        c.phase = None;
        c.offset_mm = None;
        assert!(warnings(&res_findings(&braking, "longitudinal", &c)).is_empty());
        c.setup.brakes = false;
        assert_eq!(
            warnings(&res_findings(&braking, "longitudinal", &c)).len(),
            1
        );
        assert!(warnings(&res_findings(&braking, "roll", &c)).is_empty());
    }
}
