//! Shared console/report naming for analysis metrics and warnings.
//!
//! `analyze` and `compare` both render per-domain metrics and warnings; this
//! module is the one place that decides how a `(domain, test)` pair becomes
//! a qualifier, so the two commands can never drift apart.

/// Qualified display name for a metric row: `domain::metric` or
/// `domain::test::metric`, e.g. `svc::front_left_mass_kg` or
/// `knc::vertical::bump_steer_front_left_deg_per_mm`.
pub(crate) fn qualified_metric_name(domain: &str, test: Option<&str>, metric: &str) -> String {
    format!("{}::{metric}", qualifier(domain, test))
}

/// Qualifier for a domain/test pair: `domain` or `domain::test`.
fn qualifier(domain: &str, test: Option<&str>) -> String {
    match test {
        Some(test) => format!("{domain}::{test}"),
        None => domain.into(),
    }
}

/// Tag one warning message with its domain/test qualifier, e.g.
/// `knc::vertical: metric 'bump_steer_front_left_deg_per_mm' unavailable: ...`.
pub(crate) fn tag_warning(
    domain: &str,
    test: Option<&str>,
    message: impl std::fmt::Display,
) -> String {
    format!("{}: {message}", qualifier(domain, test))
}

/// Tag every warning produced by one domain/test, in place of tagging each
/// `warnings.push` call site individually.
pub(crate) fn tag_warnings(domain: &str, test: Option<&str>, warnings: Vec<String>) -> Vec<String> {
    warnings
        .into_iter()
        .map(|message| tag_warning(domain, test, message))
        .collect()
}

/// Warning when an HDF5 signal declares no unit and a mapped unit from
/// configuration is substituted. Shared by every domain that reads HDF5
/// signals (SVC and KnC vertical).
pub(crate) fn missing_unit_warning(entity: &str, component: &str, declared_unit: &str) -> String {
    format!(
        "simulation signal '{entity}/{component}' has no HDF5 unit metadata; using mapped unit '{declared_unit}'"
    )
}

#[cfg(test)]
mod tests {
    use super::{qualified_metric_name, tag_warning};

    #[test]
    fn qualifies_metric_names_with_and_without_a_test() {
        assert_eq!(
            qualified_metric_name("svc", None, "front_left_mass_kg"),
            "svc::front_left_mass_kg"
        );
        assert_eq!(
            qualified_metric_name("knc", Some("vertical"), "bump_steer"),
            "knc::vertical::bump_steer"
        );
    }

    #[test]
    fn tags_warnings_with_the_same_qualifier_as_metrics() {
        assert_eq!(
            tag_warning("svc", None, "metric missing"),
            "svc: metric missing"
        );
        assert_eq!(
            tag_warning("knc", Some("vertical"), "metric unavailable"),
            "knc::vertical: metric unavailable"
        );
    }
}
