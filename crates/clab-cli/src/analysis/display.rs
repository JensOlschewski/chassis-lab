//! Shared console/report naming for analysis metrics and warnings.
//!
//! `analyze` and `compare` both render per-domain metrics and warnings; this
//! module is the one place that decides how a `(domain, test)` pair becomes
//! a qualifier, so the two commands can never drift apart.

use std::io::{IsTerminal, Write};

/// Keep warnings next to their metrics, but retain qualifiers in redirected output.
pub(crate) fn print_warnings(domain: &str, test: Option<&str>, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    std::io::stdout().flush().ok();
    if std::io::stdout().is_terminal() && std::io::stderr().is_terminal() {
        let prefix = format!("{}: ", qualifier(domain, test));
        eprintln!("\n  Warnings");
        for warning in warnings {
            eprintln!("    • {}", warning.strip_prefix(&prefix).unwrap_or(warning));
        }
    } else {
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
    }
}

/// Qualified display name for a metric row: `domain::metric` or
/// `domain::test::metric`, e.g. `svc::front_left_mass_kg` or
/// `knc::vertical::bump_steer_front_left_deg_per_mm`.
pub(crate) fn qualified_metric_name(domain: &str, test: Option<&str>, metric: &str) -> String {
    format!("{}::{metric}", qualifier(domain, test))
}

/// Only the concise roll summary is printed; reports retain the full catalog.
pub(crate) fn show_metric(domain: &str, test: Option<&str>, metric: &str) -> bool {
    console_metric_order(domain, test, metric).is_some()
}

/// Console order follows each KnC test's requested list; reports keep catalog order.
pub(crate) fn console_metric_order(
    domain: &str,
    test: Option<&str>,
    metric: &str,
) -> Option<usize> {
    if (domain, test) == ("knc", Some("vertical")) {
        return Some(
            corner_metric_order(
                metric,
                &[
                    ("bump_camber", "deg_per_mm", 0),
                    ("bump_steer", "deg_per_mm", 4),
                    ("bump_spin", "deg_per_mm", 8),
                    ("brake_support_angle", "deg", 12),
                    ("wheel_recession_angle", "deg", 16),
                    ("ride_frequency", "hz", 20),
                    ("ride_rate", "n_per_mm", 24),
                    ("suspension_rate", "n_per_mm", 28),
                    ("tire_rate", "n_per_mm", 32),
                ],
            )
            .unwrap_or(usize::MAX),
        );
    }
    if (domain, test) != ("knc", Some("roll")) {
        return Some(0);
    }
    corner_metric_order(
        metric,
        &[
            ("roll_camber", "deg_per_mm", 0),
            ("roll_camber", "deg_per_deg", 4),
            ("roll_steer", "deg_per_mm", 8),
            ("roll_steer", "deg_per_deg", 12),
            ("roll_suspension_rate", "n_per_mm", 16),
            ("roll_ride_rate", "n_per_mm", 20),
        ],
    )
    .or_else(|| {
        [
            "roll_center_height_front_mm",
            "roll_center_height_rear_mm",
            "roll_stiffness_front_nm_per_deg",
            "roll_stiffness_rear_nm_per_deg",
            "roll_stiffness_total_nm_per_deg",
            "roll_stiffness_distribution_front_pct",
            "roll_stiffness_distribution_rear_pct",
            "roll_stiffness_suspension_front_nm_per_deg",
            "roll_stiffness_suspension_rear_nm_per_deg",
            "roll_stiffness_suspension_total_nm_per_deg",
            "roll_stiffness_suspension_distribution_front_pct",
            "roll_stiffness_suspension_distribution_rear_pct",
        ]
        .iter()
        .position(|name| *name == metric)
        .map(|index| 24 + index)
    })
}

fn corner_metric_order(metric: &str, families: &[(&str, &str, usize)]) -> Option<usize> {
    const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
    for &(family, unit, start) in families {
        if let Some(corner) = metric
            .strip_prefix(family)
            .and_then(|rest| rest.strip_prefix('_'))
            .and_then(|rest| rest.strip_suffix(format!("_{unit}").as_str()))
            && let Some(index) = CORNERS.iter().position(|name| *name == corner)
        {
            return Some(start + index);
        }
    }
    None
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

/// Error text for a canonical signal with no entry in `mappings.toml`.
/// Shared by every required lookup that resolves a canonical name before
/// reading its raw signal (SVC and KnC vertical; simulation and
/// measurement) so the message never drifts between call sites.
pub(crate) fn mapping_missing(domain: &str, name: &str) -> String {
    format!("{domain} signal mapping missing: {name}")
}

#[cfg(test)]
mod tests {
    use super::{console_metric_order, qualified_metric_name, show_metric, tag_warning};

    #[test]
    fn roll_console_order_matches_requested_families_and_corners() {
        let rank = |name| console_metric_order("knc", Some("roll"), name).unwrap();
        assert_eq!(rank("roll_camber_front_left_deg_per_mm"), 0);
        assert_eq!(rank("roll_camber_rear_right_deg_per_deg"), 7);
        assert_eq!(rank("roll_steer_front_left_deg_per_mm"), 8);
        assert_eq!(rank("roll_steer_rear_right_deg_per_deg"), 15);
        assert_eq!(rank("roll_suspension_rate_front_left_n_per_mm"), 16);
        assert_eq!(rank("roll_ride_rate_rear_right_n_per_mm"), 23);
        assert_eq!(rank("roll_center_height_front_mm"), 24);
        assert_eq!(rank("roll_stiffness_distribution_rear_pct"), 30);
        assert_eq!(rank("roll_stiffness_suspension_front_nm_per_deg"), 31);
        assert_eq!(rank("roll_stiffness_suspension_distribution_rear_pct"), 35);
        assert_eq!(console_metric_order("svc", None, "any_metric"), Some(0));
    }

    #[test]
    fn vertical_console_order_matches_requested_families() {
        let rank = |name| console_metric_order("knc", Some("vertical"), name).unwrap();
        assert_eq!(rank("bump_camber_front_left_deg_per_mm"), 0);
        assert_eq!(rank("bump_steer_rear_right_deg_per_mm"), 7);
        assert_eq!(rank("bump_spin_front_left_deg_per_mm"), 8);
        assert_eq!(rank("brake_support_angle_front_left_deg"), 12);
        assert_eq!(rank("wheel_recession_angle_front_left_deg"), 16);
        assert_eq!(rank("ride_frequency_front_left_hz"), 20);
        assert_eq!(rank("ride_rate_front_left_n_per_mm"), 24);
        assert_eq!(rank("suspension_rate_front_left_n_per_mm"), 28);
        assert_eq!(rank("tire_rate_rear_right_n_per_mm"), 35);
    }

    #[test]
    fn console_roll_summary_does_not_filter_other_tests() {
        assert!(show_metric("svc", None, "inertia_xx_kg_mm2"));
        assert!(show_metric(
            "knc",
            Some("vertical"),
            "ride_rate_front_left_n_per_mm"
        ));
        assert!(show_metric(
            "knc",
            Some("roll"),
            "roll_ride_rate_front_left_n_per_mm"
        ));
        assert!(show_metric(
            "knc",
            Some("roll"),
            "roll_suspension_rate_rear_right_n_per_mm"
        ));
        assert!(show_metric(
            "knc",
            Some("roll"),
            "roll_stiffness_distribution_rear_pct"
        ));
        assert!(show_metric(
            "knc",
            Some("roll"),
            "roll_stiffness_suspension_distribution_rear_pct"
        ));
        assert!(show_metric(
            "knc",
            Some("roll"),
            "roll_steer_front_left_deg_per_deg"
        ));
        assert!(!show_metric(
            "knc",
            Some("roll"),
            "roll_wheel_rate_front_left_n_per_mm"
        ));
    }

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
