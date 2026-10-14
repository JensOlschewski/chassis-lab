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
pub const DOMAINS: &[(&str, &[&str])] = &[("svc", &[]), ("knc", &["vertical", "static"])];

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

/// Reject a declared identity the RES data contradicts.
pub fn check_res_identity(result: &AdamsResult, identity: &Identity, source: &str) -> Result<()> {
    if let Some(found) = res_domain(result)
        && found != identity.domain
    {
        bail!(
            "--domain {} but analysis_type in '{source}' indicates {found}",
            identity.domain
        );
    }
    Ok(())
}

/// Test an ACD `testtype` value names, if known.
fn acd_test(testtype: &str) -> Option<(&'static str, &'static str)> {
    match testtype {
        "vert" => Some(("knc", "vertical")),
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
    let Some((domain, test)) = acd_test(&info.value) else {
        return Ok(());
    };
    if domain != identity.domain {
        bail!(
            "--domain {} but testtype '{}' in '{source}' indicates {domain}",
            identity.domain,
            info.value
        );
    }
    if identity.test.as_deref() != Some(test) {
        bail!(
            "--test {} but testtype '{}' in '{source}' indicates {test}",
            identity.test.as_deref().unwrap_or("(none)"),
            info.value
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Identity, check_acd_identity, check_res_identity};
    use crate::acd::{AcdMeasurement, AcdTestInfo};
    use crate::model::{AdamsResult, Component, Entity};

    #[test]
    fn validates_domain_and_test_pairs() {
        assert!(Identity::new("svc", None).is_ok());
        assert!(Identity::new("knc", Some("vertical")).is_ok());
        assert!(Identity::new("knc", Some("static")).is_ok());
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
    fn res_identity_passes_when_analysis_type_agrees() {
        let identity = Identity::new("knc", Some("vertical")).unwrap();
        check_res_identity(&res_with_analysis_type(0.0), &identity, "source.res").unwrap();
    }

    #[test]
    fn res_identity_rejects_domain_the_analysis_type_contradicts() {
        let identity = Identity::new("svc", None).unwrap();
        let error = check_res_identity(&res_with_analysis_type(0.0), &identity, "source.res")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--domain svc"), "{error}");
        assert!(error.contains("indicates knc"), "{error}");
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
    fn acd_identity_rejects_domain_the_testtype_contradicts() {
        let identity = Identity::new("svc", None).unwrap();
        let error = check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--domain svc"), "{error}");
        assert!(error.contains("indicates knc"), "{error}");
    }

    #[test]
    fn acd_identity_rejects_test_the_testtype_contradicts() {
        let identity = Identity::new("knc", Some("static")).unwrap();
        let error = check_acd_identity(&acd_with_testtype("vert"), &identity, "source.acd")
            .unwrap_err()
            .to_string();
        assert!(error.contains("--test static"), "{error}");
        assert!(error.contains("indicates vertical"), "{error}");
    }
}
