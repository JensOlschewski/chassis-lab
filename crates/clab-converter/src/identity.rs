//! Domain and test a converted file belongs to.
//!
//! The caller declares both. The data is only used to cross-check the
//! declaration, which produces warnings and never changes what is written.

use anyhow::{Result, bail};

use crate::acd::AcdMeasurement;
use crate::model::AdamsResult;

/// Known domains and their tests. Add new domains and tests here.
pub const DOMAINS: &[(&str, &[&str])] = &[("svc", &[]), ("knc", &["vertical"])];

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

/// Warnings for a declared identity the RES data contradicts.
pub fn res_warnings(result: &AdamsResult, identity: &Identity, source: &str) -> Vec<String> {
    match res_domain(result) {
        Some(found) if found != identity.domain => vec![format!(
            "--domain {} but analysis_type in '{source}' indicates {found}",
            identity.domain
        )],
        _ => Vec::new(),
    }
}

/// Test an ACD `testtype` value names, if known.
fn acd_test(testtype: &str) -> Option<(&'static str, &'static str)> {
    match testtype {
        "vert" => Some(("knc", "vertical")),
        _ => None,
    }
}

/// Warnings for a declared identity the ACD `testtype` contradicts.
pub fn acd_warnings(
    measurement: &AcdMeasurement,
    identity: &Identity,
    source: &str,
) -> Vec<String> {
    let Some(info) = measurement
        .test_info
        .iter()
        .find(|info| info.name == "testtype")
    else {
        return Vec::new();
    };
    let Some((domain, test)) = acd_test(&info.value) else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    if domain != identity.domain {
        warnings.push(format!(
            "--domain {} but testtype '{}' in '{source}' indicates {domain}",
            identity.domain, info.value
        ));
    } else if identity.test.as_deref() != Some(test) {
        warnings.push(format!(
            "--test {} but testtype '{}' in '{source}' indicates {test}",
            identity.test.as_deref().unwrap_or("(none)"),
            info.value
        ));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::Identity;

    #[test]
    fn validates_domain_and_test_pairs() {
        assert!(Identity::new("svc", None).is_ok());
        assert!(Identity::new("knc", Some("vertical")).is_ok());
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
}
