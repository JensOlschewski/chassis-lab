//! Human-readable index and compact counts for report bundles.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Default, Serialize)]
struct Counts {
    total: usize,
    passed: usize,
    failed: usize,
    missing: usize,
    informational: usize,
    not_compared_domains: usize,
}

impl Counts {
    fn add_metric(&mut self, metric: &Value) {
        self.total += 1;
        match metric["status"].as_str() {
            Some("passed") => self.passed += 1,
            Some("failed") => self.failed += 1,
            Some("missing") => self.missing += 1,
            _ => self.informational += 1,
        }
    }

    fn add(&mut self, other: &Self) {
        self.total += other.total;
        self.passed += other.passed;
        self.failed += other.failed;
        self.missing += other.missing;
        self.informational += other.informational;
        self.not_compared_domains += other.not_compared_domains;
    }
}

/// Enrich the canonical JSON and add an index without changing domain details.
pub(crate) fn prepare(mut files: Vec<(&str, Vec<u8>)>) -> Result<Vec<(&str, Vec<u8>)>> {
    let report_file = files
        .iter_mut()
        .find(|(name, _)| *name == "report.json")
        .expect("report bundle has report.json");
    let mut report: Value = serde_json::from_slice(&report_file.1)?;
    add_summary(&mut report);
    report_file.1 = (serde_json::to_string_pretty(&report)? + "\n").into_bytes();
    let html = overview(&report, &files);
    files.push(("index.html", html.into_bytes()));
    Ok(files)
}

fn add_summary(report: &mut Value) {
    if report["command"] == "analyze" {
        let metrics = report["metrics"].as_array().expect("analysis metrics");
        report["summary"] = json!({
            "metric_count": metrics.len(),
            "missing_count": metrics.iter().filter(|metric| metric["value"].is_null()).count(),
            "warning_count": report["warnings"].as_array().expect("analysis warnings").len(),
        });
        return;
    }

    let mut overall = Counts::default();
    for candidate in report["candidates"]
        .as_array_mut()
        .expect("comparison candidates")
    {
        let mut counts = Counts::default();
        let mut groups: BTreeMap<(String, Option<String>), Counts> = BTreeMap::new();
        for metric in candidate["metrics"].as_array().expect("comparison metrics") {
            counts.add_metric(metric);
            groups
                .entry((
                    metric["domain"].as_str().unwrap_or_default().to_owned(),
                    metric["test"].as_str().map(str::to_owned),
                ))
                .or_default()
                .add_metric(metric);
        }
        for domain in candidate["domains"].as_array().expect("comparison domains") {
            if domain["status"] == "not compared" {
                counts.not_compared_domains += 1;
                groups
                    .entry((
                        domain["domain"].as_str().unwrap_or_default().to_owned(),
                        domain["test"].as_str().map(str::to_owned),
                    ))
                    .or_default()
                    .not_compared_domains += 1;
            }
        }
        overall.add(&counts);
        let by_domain = groups
            .into_iter()
            .map(|((domain, test), counts)| json!({"domain": domain, "test": test, "counts": counts}))
            .collect::<Vec<_>>();
        candidate["summary"] = json!({"counts": counts, "by_domain": by_domain});
    }
    report["summary"] = json!({"overall": overall});
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn overview(report: &Value, files: &[(&str, Vec<u8>)]) -> String {
    let mut html = String::from(concat!(
        "<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\">",
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
        "<title>Chassis Lab report</title>\n<style>",
        "body{font:1rem system-ui,sans-serif;line-height:1.5;max-width:68rem;margin:2rem auto;padding:0 1rem;color:#202936}",
        "a{color:#145a86}table{border-collapse:collapse;width:100%}th,td{text-align:left;padding:.4rem .7rem;border-bottom:1px solid #ccd3dc}",
        "th{background:#eef2f6}section{margin:2rem 0}code{overflow-wrap:anywhere}li{margin:.25rem 0}",
        "</style><main><h1>Chassis Lab report</h1>"
    ));
    if report["command"] == "analyze" {
        let case = &report["case"];
        html.push_str(&format!(
            "<p>Analysis: <strong>{}</strong> ({})</p><p>{} metrics · {} missing · {} warnings</p>",
            escape(case["name"].as_str().unwrap_or_default()),
            escape(case["loadcase"].as_str().unwrap_or_default()),
            report["summary"]["metric_count"],
            report["summary"]["missing_count"],
            report["summary"]["warning_count"],
        ));
    } else {
        let counts = &report["summary"]["overall"];
        html.push_str(&format!(
            "<p>Comparison: <strong>{}</strong> · baseline {}</p>",
            escape(report["status"].as_str().unwrap_or_default()),
            escape(
                report["baseline"]["case"]["name"]
                    .as_str()
                    .unwrap_or_default()
            ),
        ));
        html.push_str("<section><h2>Results</h2><table><thead><tr><th>Candidate</th><th>Passed</th><th>Failed</th><th>Missing</th><th>Informational</th><th>Not compared domains</th></tr></thead><tbody>");
        for candidate in report["candidates"].as_array().unwrap() {
            let c = &candidate["summary"]["counts"];
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(candidate["case"]["name"].as_str().unwrap_or_default()),
                c["passed"],
                c["failed"],
                c["missing"],
                c["informational"],
                c["not_compared_domains"]
            ));
        }
        html.push_str(&format!(
            "</tbody></table><p>Total: {} passed · {} failed · {} missing · {} informational · {} domains not compared</p></section>",
            counts["passed"], counts["failed"], counts["missing"], counts["informational"], counts["not_compared_domains"]
        ));
    }
    html.push_str("<p>Data: <a href=\"report.json\">report.json</a> · <a href=\"metrics.csv\">metrics.csv</a></p>");

    let mut plots: BTreeMap<(&str, bool), Vec<&str>> = BTreeMap::new();
    for (name, _) in files {
        if let Some(rest) = name.strip_prefix("plots/knc/")
            && name.ends_with(".pdf")
            && let Some(test) = rest.split('/').next()
        {
            plots
                .entry((test, name.contains("/debug/")))
                .or_default()
                .push(name);
        }
    }
    if !plots.is_empty() {
        html.push_str("<section><h2>Plots</h2>");
        for ((test, debug), paths) in plots {
            let title = if debug { " debug plots" } else { " plots" };
            html.push_str(&format!(
                "<details{}><summary>{}{title} ({})</summary><ul>",
                if debug { "" } else { " open" },
                escape(test),
                paths.len(),
            ));
            for path in paths {
                let label = path
                    .rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .trim_end_matches(".pdf")
                    .replace('_', " ");
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    escape(path),
                    escape(&label)
                ));
            }
            html.push_str("</ul></details>");
        }
        html.push_str("</section>");
    }
    html.push_str("</main></html>\n");
    html
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use serde_json::{Value, json};

    #[test]
    fn summarizes_missing_domains_and_links_only_generated_plots() {
        let report = json!({
            "command": "compare", "status": "incomplete", "baseline": {"case": {"name": "base"}},
            "candidates": [{
                "case": {"name": "<candidate>"},
                "metrics": [{"domain": "svc", "test": null, "status": "passed"},
                            {"domain": "svc", "test": null, "status": "missing"}],
                "domains": [{"domain": "svc", "test": null, "status": "compared"},
                            {"domain": "knc", "test": "roll", "status": "not compared"}]
            }, {
                "case": {"name": "second"},
                "metrics": [{"domain": "svc", "test": null, "status": "failed"}],
                "domains": [{"domain": "svc", "test": null, "status": "compared"}]
            }]
        });
        let files = prepare(vec![
            ("report.json", serde_json::to_vec(&report).unwrap()),
            ("plots/knc/roll/comparison/roll.pdf", vec![]),
            ("plots/knc/roll/comparison/debug/rate.pdf", vec![]),
        ])
        .unwrap();
        let summary: Value = serde_json::from_slice(&files[0].1).unwrap();
        assert_eq!(summary["summary"]["overall"]["total"], 3);
        assert_eq!(summary["summary"]["overall"]["missing"], 1);
        assert_eq!(summary["summary"]["overall"]["failed"], 1);
        assert_eq!(summary["summary"]["overall"]["not_compared_domains"], 1);
        assert_eq!(
            summary["candidates"][0]["summary"]["by_domain"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let html = String::from_utf8(files.last().unwrap().1.clone()).unwrap();
        assert!(html.contains("&lt;candidate&gt;"));
        assert!(html.contains("href=\"plots/knc/roll/comparison/roll.pdf\""));
        assert!(html.contains("roll debug plots"));
        assert!(html.contains("<details><summary>roll debug plots"));
    }
}
