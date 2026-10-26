use std::{
    collections::BTreeMap,
    env, fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use clab_core::svc_reference::load_svc_reference;
use clap::Args;
use sha2::{Digest, Sha256};

use crate::analysis::{
    knc::{self, KncTest},
    svc,
};

#[derive(Debug, Args)]
pub(crate) struct AnalyzeArgs {
    /// Shared TOML configuration: mappings, units, conventions, physics, and setup.
    config: PathBuf,
    /// Normalized input file or run TOML manifest.
    input: PathBuf,
    /// Directory for deterministic report bundle.
    #[arg(short = 'r', long = "report")]
    output: Option<PathBuf>,
    /// Replace report files already in output directory.
    #[arg(long)]
    force: bool,
    /// Include KnC rate-filter diagnostic plots in the report (requires --report).
    #[arg(long, requires = "output")]
    debug: bool,
    /// Print individual analysis warnings.
    #[arg(short, long)]
    verbose: bool,
}

struct SvcReferenceReports {
    report: serde_json::Value,
    detail: String,
    csv: String,
    warnings: Vec<String>,
}

pub(crate) fn analyze(args: AnalyzeArgs) -> Result<()> {
    let extension = args
        .input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("toml") => {
            if !args.input.is_file() {
                bail!("run manifest cannot be found: {}", args.input.display());
            }
            analyze_run(&args)
        }
        Some("json") => {
            validate_input_file(&args.input)?;
            analyze_svc(&args)
        }
        Some("h5" | "hdf5") => {
            validate_input_file(&args.input)?;
            analyze_hdf5(&args)
        }
        _ => bail!("analyze accepts run TOML, SVC JSON, or normalized HDF5 input"),
    }
}

fn analyze_run(args: &AnalyzeArgs) -> Result<()> {
    clab_core::config::validate_run_input_files(&args.input).map_err(|error| {
        anyhow::anyhow!(
            "failed to load config/run '{}': {error}",
            args.input.display()
        )
    })?;
    let config =
        clab_core::config::load_config_and_run(&args.config, &args.input).map_err(|error| {
            anyhow::anyhow!(
                "failed to load config '{}' with run '{}': {error}",
                args.config.display(),
                args.input.display()
            )
        })?;
    let has_svc = !config.simulation_json.as_os_str().is_empty();
    let mut sources = Vec::new();
    if has_svc && is_hdf5(&config.simulation_json) {
        sources.push(config.simulation_json.as_path());
    }
    for test in [KncTest::Vertical, KncTest::Roll] {
        if let Some(knc) = test.config(&config) {
            sources.push(knc_input(&config, test).expect("KnC input path"));
            if let Some(static_input) = &knc.static_simulation_hdf5 {
                sources.push(static_input);
            }
        }
    }
    for path in sources {
        if let Some(warning) = CanonicalFile::open(path)?.summary().legacy_warning() {
            eprintln!("warning: {warning}");
        }
    }
    let knc_tests = [KncTest::Vertical, KncTest::Roll]
        .into_iter()
        .filter_map(|test| knc_input(&config, test).map(|path| (test, path)))
        .collect::<Vec<_>>();
    match (has_svc, knc_tests.as_slice()) {
        (false, []) => Ok(()),
        (false, [(test, path)]) => write_knc(&config, *test, path, args, Some(&args.input)),
        (true, []) if is_hdf5(&config.simulation_json) => {
            if config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement) {
                bail!(
                    "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
                    config.simulation_json.display()
                );
            }
            write_svc_hdf5_run(&config, args, &args.input)
        }
        (true, []) => match config.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) => {
                write_svc(&config, args, Some(&args.input))
            }
            Some(clab_core::config::SourceKind::Measurement) => {
                let reference =
                    load_svc_reference(&config.simulation_json, &config.svc_measurement_signals)
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                write_svc_reference(&reference, args, &config, Some(&args.input))
            }
            None => unreachable!(),
        },
        _ => write_combined_run(&config, has_svc, &knc_tests, args),
    }
}

/// Configured input file of one KnC test, `None` when the test is absent.
fn knc_input(config: &clab_core::config::CaseConfig, test: KncTest) -> Option<&Path> {
    test.config(config).map(|knc| match knc.source_kind {
        clab_core::config::SourceKind::Simulation => knc.simulation_hdf5.as_path(),
        clab_core::config::SourceKind::Measurement => knc.measurement_hdf5.as_path(),
    })
}

fn validate_input_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("input file cannot be found: {}", path.display());
    }
    Ok(())
}

/// Run manifest with more than one domain: optional SVC plus any KnC tests.
fn write_combined_run(
    config: &clab_core::config::CaseConfig,
    has_svc: bool,
    knc_tests: &[(KncTest, &Path)],
    args: &AnalyzeArgs,
) -> Result<()> {
    let svc_reports = if has_svc {
        Some(svc_run_reports(config, args)?)
    } else {
        None
    };
    // SVC masses back the vertical `ride_frequency` metric.
    let svc_corner_mass_kg = svc_reports
        .as_ref()
        .map(|reports| svc_corner_mass_kg(&reports.report));
    let mut warnings = svc_reports
        .as_ref()
        .map(|reports| reports.warnings.clone())
        .unwrap_or_default();
    let mut knc_reports = Vec::new();
    for &(test, path) in knc_tests {
        let debug = debug_info(args.debug, &args.config, &[path])?;
        let report = knc::analyze_input(
            config,
            test,
            path,
            svc_corner_mass_kg.as_ref(),
            debug.as_deref(),
        )?;
        warnings.extend(report.warnings.iter().cloned());
        knc_reports.push((test, path, report, debug));
    }
    warnings.sort();
    warnings.dedup();
    if let Some(reports) = &svc_reports {
        print_header("SVC", reports.metric_count);
        print_metrics(reports.report["metrics"].as_array().unwrap());
    }
    for (test, _, report, _) in &knc_reports {
        print_header(test.title(), visible_metric_count(&report.metrics));
        print_metrics(&report.metrics);
    }
    print_warnings(&warnings, args);
    let Some(output) = &args.output else {
        return Ok(());
    };
    let mut domains = Vec::new();
    let mut metrics = Vec::new();
    let mut csv = "run,domain,test,metric,unit,value\n".to_owned();
    if let Some(reports) = &svc_reports {
        domains.push(serde_json::json!({"domain": "svc", "metric_count": reports.metric_count}));
        metrics.extend(
            reports.report["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
        );
        csv = reports.csv.clone();
    }
    for (test, _, report, _) in &knc_reports {
        domains.push(knc_domain(*test, report));
        metrics.extend(report.metrics.iter().cloned());
        csv += &knc_csv_rows(&config.name, *test, &report.metrics)?;
    }
    let bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "case": {"name": config.name, "loadcase": config.loadcase},
        "inputs": {"config": provenance(&args.config)?, "run": provenance(&args.input)?},
        "domains": domains,
        "metrics": metrics,
        "warnings": warnings,
    });
    let mut files = vec![
        (
            "report.json".to_owned(),
            (serde_json::to_string_pretty(&bundle)? + "\n").into_bytes(),
        ),
        ("metrics.csv".to_owned(), csv.into_bytes()),
    ];
    if let Some(reports) = &svc_reports {
        let mut svc_detail: serde_json::Value = serde_json::from_str(&reports.detail)?;
        svc_detail["case"] = bundle["case"].clone();
        svc_detail["inputs"]["run"] = bundle["inputs"]["run"].clone();
        files.push((
            "svc.json".to_owned(),
            (serde_json::to_string_pretty(&svc_detail)? + "\n").into_bytes(),
        ));
    }
    let mut inputs = vec![args.config.as_path(), args.input.as_path()];
    if has_svc {
        inputs.push(config.simulation_json.as_path());
    }
    for (test, path, report, debug) in &knc_reports {
        let mut detail: serde_json::Value = serde_json::from_str(&report.detail)?;
        let plots = knc_plots(
            config,
            *test,
            &detail,
            report,
            &analysis_footer(&args.config, config, *test, path, Some(&args.input))?,
            debug.as_deref(),
        )?;
        detail["case"] = bundle["case"].clone();
        let mut knc_inputs = bundle["inputs"].clone();
        knc_inputs["input"] = provenance(path)?;
        if let Some(static_input) = test
            .config(config)
            .and_then(|knc| knc.static_simulation_hdf5.as_ref())
        {
            knc_inputs["static_simulation"] = provenance(static_input)?;
        }
        detail["inputs"] = knc_inputs;
        files.push((
            format!("knc-{}.json", test.name()),
            (serde_json::to_string_pretty(&detail)? + "\n").into_bytes(),
        ));
        files.extend(plots);
        inputs.push(path);
        add_knc_inputs(&mut inputs, config, *test);
    }
    let files = files
        .iter()
        .map(|(name, contents)| (name.as_str(), contents.clone()))
        .collect::<Vec<_>>();
    write_bundle(output, &files, args.force, &inputs)?;
    println!("report: {}", output.display());
    Ok(())
}

/// SVC reports of a run manifest from its simulation input or measurement reference.
fn svc_run_reports(
    config: &clab_core::config::CaseConfig,
    args: &AnalyzeArgs,
) -> Result<svc::Reports> {
    if is_hdf5(&config.simulation_json) {
        if config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement) {
            bail!(
                "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
                config.simulation_json.display()
            );
        }
        return svc::analyze_hdf5(config);
    }
    match config.svc_source_kind {
        Some(clab_core::config::SourceKind::Simulation) => svc::analyze(config),
        Some(clab_core::config::SourceKind::Measurement) => {
            let reference =
                load_svc_reference(&config.simulation_json, &config.svc_measurement_signals)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            let reports = svc_reference_reports(&reference, &args.config, config)?;
            Ok(svc::Reports {
                metric_count: reports.report["metrics"].as_array().unwrap().len(),
                report: reports.report,
                detail: reports.detail,
                csv: reports.csv,
                warnings: reports.warnings,
            })
        }
        None => unreachable!(),
    }
}

/// `domains` entry of one KnC test in `report.json`.
fn knc_domain(test: KncTest, report: &knc::Reports) -> serde_json::Value {
    serde_json::json!({
        "domain": "knc",
        "test": test.name(),
        "metric_count": report.metrics.len(),
        "curve_count": report.curve_count,
        "normalization": report.normalization,
    })
}

/// Corner PDF plots of one KnC test; roll axle plots arrive with #144.
fn knc_plots(
    config: &clab_core::config::CaseConfig,
    test: KncTest,
    detail: &serde_json::Value,
    report: &knc::Reports,
    footer: &str,
    debug: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    knc::curve_plots(
        config,
        test,
        &detail["curves"],
        footer,
        report.measurement,
        &report.corner_mass_kg,
        debug,
    )
}

/// SVC's `<corner>_mass_kg` metrics, keyed by corner — the `ride_frequency`
/// vertical metric's default mass source when both domains run together.
fn svc_corner_mass_kg(svc_report: &serde_json::Value) -> BTreeMap<String, f64> {
    const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
    svc_report["metrics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|metric| {
            let name = metric["metric"].as_str()?;
            let corner = name.strip_suffix("_mass_kg")?;
            if !CORNERS.contains(&corner) {
                return None;
            }
            metric["value"]
                .as_f64()
                .map(|value| (corner.to_string(), value))
        })
        .collect()
}

fn is_hdf5(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("h5" | "hdf5")
    )
}

fn analyze_svc(args: &AnalyzeArgs) -> Result<()> {
    let input: serde_json::Value = serde_json::from_reader(
        fs::File::open(&args.input)
            .with_context(|| format!("failed to read JSON input '{}'", args.input.display()))?,
    )
    .with_context(|| format!("failed to parse JSON input '{}'", args.input.display()))?;
    match input.get("source_kind").and_then(serde_json::Value::as_str) {
        Some("measurement") => {
            let config = clab_core::config::load_config_with_input(&args.config, &args.input, None)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to load analysis config '{}': {error}",
                        args.config.display()
                    )
                })?;
            let reference = load_svc_reference(&args.input, &config.svc_measurement_signals)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            write_svc_reference(&reference, args, &config, None)
        }
        Some("simulation") => {
            let config = clab_core::config::load_config_with_input(&args.config, &args.input, None)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to load analysis config '{}': {error}",
                        args.config.display()
                    )
                })?;
            write_svc(&config, args, None)
        }
        None => bail!(
            "input '{}': source_kind is required; allowed values: simulation, measurement",
            args.input.display()
        ),
        Some(value) => bail!(
            "input '{}': unsupported source_kind '{value}'; allowed values: simulation, measurement",
            args.input.display()
        ),
    }
}

fn analyze_hdf5(args: &AnalyzeArgs) -> Result<()> {
    let input = CanonicalFile::open(&args.input)
        .with_context(|| format!("failed to read HDF5 '{}'", args.input.display()))?;
    let summary = input.summary();
    let svc_measurement_error = || {
        anyhow::anyhow!(
            "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
            args.input.display()
        )
    };
    match (summary.domain.as_deref(), summary.test.as_deref()) {
        (Some("knc"), Some(test)) if KncTest::from_name(test).is_some() => {
            analyze_knc(args, KncTest::from_name(test).expect("known KnC test"))
        }
        (Some("knc"), test) => bail!(
            "unsupported knc test {} in '{}'; supported: vertical, roll",
            test.map_or("(none)".into(), |test| format!("'{test}'")),
            args.input.display()
        ),
        (Some("svc"), _) => {
            if summary.source_kind == SourceKind::Measurement {
                return Err(svc_measurement_error());
            }
            let config = clab_core::config::load_config_with_input(&args.config, &args.input, None)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to load analysis config '{}': {error}",
                        args.config.display()
                    )
                })?;
            write_svc_hdf5(&config, args)
        }
        (Some(domain), _) => bail!(
            "unsupported domain '{domain}' in '{}'; supported: svc, knc",
            args.input.display()
        ),
        (None, _) => {
            // Converted before domain and test were stored: infer from the config.
            if let Some(warning) = summary.legacy_warning() {
                eprintln!("warning: {warning}");
            }
            if summary.source_kind == SourceKind::Measurement
                && let Ok(config) =
                    clab_core::config::load_config_with_input(&args.config, &args.input, None)
                && config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement)
            {
                return Err(svc_measurement_error());
            }
            if summary.source_kind == SourceKind::Simulation
                && let Ok(config) =
                    clab_core::config::load_config_with_input(&args.config, &args.input, None)
                && svc::supports_hdf5(&config, &input)
            {
                return write_svc_hdf5(&config, args);
            }
            analyze_knc(args, KncTest::Vertical)
        }
    }
}

fn write_svc_reference(
    reference: &clab_core::svc_reference::SvcReference,
    args: &AnalyzeArgs,
    config: &clab_core::config::CaseConfig,
    run: Option<&Path>,
) -> Result<()> {
    let mut reports = svc_reference_reports(reference, &args.config, config)?;
    if let Some(run) = run {
        reports.report["inputs"]["run"] = provenance(run)?;
        reports.detail = serde_json::to_string_pretty(&reports.report)? + "\n";
    }
    print_header("SVC", reports.report["metrics"].as_array().unwrap().len());
    print_metrics(reports.report["metrics"].as_array().unwrap());
    print_warnings(&reports.warnings, args);
    if let Some(output) = &args.output {
        write_bundle(
            output,
            &[
                ("report.json", reports.detail.clone().into_bytes()),
                ("metrics.csv", reports.csv.into_bytes()),
                ("svc.json", reports.detail.into_bytes()),
            ],
            args.force,
            &[&args.config, &args.input, &reference.path],
        )?;
        println!("report: {}", output.display());
    }
    Ok(())
}

fn svc_reference_reports(
    reference: &clab_core::svc_reference::SvcReference,
    config: &Path,
    case: &clab_core::config::CaseConfig,
) -> Result<SvcReferenceReports> {
    let mut warnings = Vec::new();
    let metrics = svc::canonical_metrics(&reference.values)
        .into_iter()
        .map(|metric| {
            let value = metric.value;
            if value.is_none() {
                warnings.push(format!(
                    "metric '{}' is missing from measurement reference",
                    metric.name
                ));
            }
            serde_json::json!({
                "domain": "svc", "test": null, "metric": metric.name, "unit": metric.unit, "value": value,
            })
        })
        .collect::<Vec<_>>();
    let warnings = crate::analysis::display::tag_warnings("svc", None, warnings);
    let report = serde_json::json!({
        "schema_version": 1, "command": "analyze", "case": {"name": case.name, "loadcase": case.loadcase}, "inputs": {"config": provenance(config)?, "reference": provenance(&reference.path)?}, "metrics": metrics, "warnings": warnings,
    });
    let mut csv = csv::Writer::from_writer(Vec::new());
    csv.write_record(["run", "domain", "test", "metric", "unit", "value"])?;
    for metric in report["metrics"].as_array().unwrap() {
        let value = metric["value"]
            .as_f64()
            .map(|value| value.to_string())
            .unwrap_or_default();
        csv.write_record([
            "reference",
            "svc",
            "",
            metric["metric"].as_str().unwrap(),
            metric["unit"].as_str().unwrap(),
            value.as_str(),
        ])?;
    }
    Ok(SvcReferenceReports {
        detail: serde_json::to_string_pretty(&report)? + "\n",
        report,
        csv: String::from_utf8(csv.into_inner()?)?,
        warnings,
    })
}

fn write_svc(
    config: &clab_core::config::CaseConfig,
    args: &AnalyzeArgs,
    run: Option<&Path>,
) -> Result<()> {
    write_svc_report(config, args, run, svc::analyze(config)?)
}

fn write_svc_hdf5(config: &clab_core::config::CaseConfig, args: &AnalyzeArgs) -> Result<()> {
    write_svc_report(config, args, None, svc::analyze_hdf5(config)?)
}

fn write_svc_hdf5_run(
    config: &clab_core::config::CaseConfig,
    args: &AnalyzeArgs,
    run: &Path,
) -> Result<()> {
    write_svc_report(config, args, Some(run), svc::analyze_hdf5(config)?)
}

fn write_svc_report(
    config: &clab_core::config::CaseConfig,
    args: &AnalyzeArgs,
    run: Option<&Path>,
    report: svc::Reports,
) -> Result<()> {
    print_header("SVC", report.metric_count);
    print_metrics(report.report["metrics"].as_array().unwrap());
    print_warnings(&report.warnings, args);
    if let Some(output) = &args.output {
        let mut bundle = report.report;
        if let Some(run) = run {
            bundle["inputs"]["run"] = provenance(run)?;
        }
        let detail = serde_json::to_string_pretty(&bundle)? + "\n";
        write_bundle(
            output,
            &[
                ("report.json", detail.clone().into_bytes()),
                ("metrics.csv", report.csv.into_bytes()),
                ("svc.json", detail.into_bytes()),
            ],
            args.force,
            &[&args.config, &args.input, &config.simulation_json],
        )?;
        println!("report: {}", output.display());
    }
    Ok(())
}

fn print_metrics(metrics: &[serde_json::Value]) {
    let mut visible: Vec<_> = visible_metrics(metrics).collect();
    visible.sort_by_key(|metric| {
        crate::analysis::display::console_metric_order(
            metric["domain"].as_str().unwrap_or_default(),
            metric["test"].as_str(),
            metric["metric"].as_str().unwrap(),
        )
    });
    for metric in visible {
        let value = if metric["value"].is_null() {
            if std::io::stdout().is_terminal() {
                "\x1b[38;5;208mmissing\x1b[0m".into()
            } else {
                "missing".into()
            }
        } else {
            metric["value"].to_string()
        };
        let name = crate::analysis::display::qualified_metric_name(
            metric["domain"].as_str().unwrap_or_default(),
            metric["test"].as_str(),
            metric["metric"].as_str().unwrap(),
        );
        println!("  {name} ... {value} {}", metric["unit"].as_str().unwrap());
    }
}

fn visible_metrics(metrics: &[serde_json::Value]) -> impl Iterator<Item = &serde_json::Value> {
    metrics.iter().filter(|metric| {
        crate::analysis::display::show_metric(
            metric["domain"].as_str().unwrap_or_default(),
            metric["test"].as_str(),
            metric["metric"].as_str().unwrap_or_default(),
        )
    })
}

fn visible_metric_count(metrics: &[serde_json::Value]) -> usize {
    visible_metrics(metrics).count()
}

fn print_header(title: &str, metric_count: usize) {
    println!();
    println!("{title}");
    println!("running {metric_count} metrics");
}

fn print_warnings(warnings: &[String], args: &AnalyzeArgs) {
    if warnings.is_empty() {
        return;
    }
    if args.verbose {
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
        return;
    }
    let suffix = if warnings.len() == 1 { "" } else { "s" };
    if let Some(output) = &args.output {
        eprintln!(
            "warning: {} analysis warning{suffix}; details in {}/report.json",
            warnings.len(),
            output.display()
        );
    } else {
        eprintln!(
            "warning: {} analysis warning{suffix}; rerun with --verbose for details",
            warnings.len()
        );
    }
}

fn analyze_knc(args: &AnalyzeArgs, test: KncTest) -> Result<()> {
    let config =
        clab_core::config::load_config_with_input(&args.config, &args.input, Some(test.name()))
            .map_err(|error| {
                anyhow::anyhow!(
                    "failed to load analysis config '{}': {error}",
                    args.config.display()
                )
            })?;
    write_knc(&config, test, &args.input, args, None)
}

fn write_knc(
    config: &clab_core::config::CaseConfig,
    test: KncTest,
    input: &Path,
    args: &AnalyzeArgs,
    run: Option<&Path>,
) -> Result<()> {
    let footer = analysis_footer(&args.config, config, test, input, run)?;
    let debug = debug_info(args.debug, &args.config, &[input])?;
    let report = knc::analyze_input(config, test, input, None, debug.as_deref())?;
    print_header(test.title(), visible_metric_count(&report.metrics));
    print_metrics(&report.metrics);
    print_warnings(&report.warnings, args);
    let Some(output) = &args.output else {
        return Ok(());
    };
    let mut bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "case": {"name": config.name, "loadcase": config.loadcase},
        "inputs": {"config": provenance(&args.config)?, "input": provenance(input)?},
        "domains": [knc_domain(test, &report)],
        "metrics": report.metrics,
        "warnings": report.warnings,
    });
    if let Some(run) = run {
        bundle["inputs"]["run"] = provenance(run)?;
    }
    let mut detail: serde_json::Value = serde_json::from_str(&report.detail)?;
    let plots = knc_plots(config, test, &detail, &report, &footer, debug.as_deref())?;
    detail["case"] = bundle["case"].clone();
    detail["inputs"] = bundle["inputs"].clone();
    let detail = serde_json::to_string_pretty(&detail)? + "\n";
    let detail_name = format!("knc-{}.json", test.name());
    let mut files = vec![
        (
            "report.json",
            (serde_json::to_string_pretty(&bundle)? + "\n").into_bytes(),
        ),
        (
            "metrics.csv",
            ("run,domain,test,metric,unit,value\n".to_owned()
                + &knc_csv_rows(&config.name, test, &report.metrics)?)
                .into_bytes(),
        ),
        (detail_name.as_str(), detail.into_bytes()),
    ];
    files.extend(
        plots
            .iter()
            .map(|(name, contents)| (name.as_str(), contents.clone())),
    );
    let mut inputs = vec![args.config.as_path(), input];
    add_knc_inputs(&mut inputs, config, test);
    if let Some(run) = run {
        inputs.push(run);
    }
    write_bundle(output, &files, args.force, &inputs)?;
    println!("report: {}", output.display());
    Ok(())
}

/// Metric rows in shared analysis CSV layout, without header.
fn knc_csv_rows(run: &str, test: KncTest, metrics: &[serde_json::Value]) -> Result<String> {
    let mut csv = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    for metric in metrics {
        csv.write_record([
            crate::compare::safe_cell(run),
            "knc".into(),
            test.name().into(),
            crate::compare::safe_cell(metric["metric"].as_str().unwrap_or_default()),
            metric["unit"].as_str().unwrap_or_default().into(),
            metric["value"]
                .as_f64()
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ])?;
    }
    Ok(String::from_utf8(csv.into_inner()?)?)
}

fn add_knc_inputs<'a>(
    inputs: &mut Vec<&'a Path>,
    config: &'a clab_core::config::CaseConfig,
    test: KncTest,
) {
    let Some(knc) = test.config(config) else {
        return;
    };
    inputs.push(&knc.simulation_hdf5);
    inputs.push(&knc.measurement_hdf5);
    if let Some(static_simulation) = &knc.static_simulation_hdf5 {
        inputs.push(static_simulation);
    }
}

fn analysis_footer(
    config_path: &Path,
    config: &clab_core::config::CaseConfig,
    test: KncTest,
    input: &Path,
    run: Option<&Path>,
) -> Result<String> {
    let mut lines = Vec::new();
    if let Some(knc) = test.config(config) {
        lines.push(format!("KnC setup: {}", knc.setup.summary()));
    }
    lines.push(format!("Config: {}", absolute_path(config_path)?.display()));
    lines.push(format!("Case: {} | {}", config.name, config.loadcase));
    lines.push(format!("Input: {}", absolute_path(input)?.display()));
    if let Some(run) = run {
        lines.push(format!("Run: {}", absolute_path(run)?.display()));
    }
    lines.push(format!("Created by: {}", report_creator()));
    lines.push(format!("clab version: {}", env!("CARGO_PKG_VERSION")));
    Ok(lines.join("\n"))
}

/// JSON for the Python debug plots: config path, creator, clab version and one input path per series.
pub(crate) fn debug_info(enabled: bool, config: &Path, inputs: &[&Path]) -> Result<Option<String>> {
    if !enabled {
        return Ok(None);
    }
    let inputs = inputs
        .iter()
        .map(|path| absolute_path(path))
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(
        serde_json::json!({
            "config": absolute_path(config)?,
            "creator": report_creator(),
            "version": env!("CARGO_PKG_VERSION"),
            "inputs": inputs,
        })
        .to_string(),
    ))
}

/// Absolute path without `.` components or `..` hops, without touching the
/// file system (symlinks stay as written).
pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

fn report_creator() -> String {
    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".into());
    let host = env::var("HOSTNAME").unwrap_or_else(|_| {
        Command::new("hostname")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown-host".into())
    });
    format!("{user}@{host}")
}

fn provenance(path: &Path) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"path": path, "sha256": format!("{:x}", Sha256::digest(fs::read(path)?))}),
    )
}

pub(crate) fn write_bundle(
    output: &Path,
    files: &[(&str, Vec<u8>)],
    force: bool,
    inputs: &[&Path],
) -> Result<()> {
    let output = output
        .canonicalize()
        .unwrap_or_else(|_| output.to_path_buf());
    for (name, _) in files {
        let destination = output.join(name);
        if inputs.iter().any(|input| {
            input
                .canonicalize()
                .unwrap_or_else(|_| (*input).to_path_buf())
                == destination
        }) {
            bail!(
                "report output must not replace an input: {}",
                destination.display()
            );
        }
    }
    if force && output.is_dir() {
        for input in inputs {
            let input = input
                .canonicalize()
                .unwrap_or_else(|_| (*input).to_path_buf());
            if input.starts_with(&output) {
                bail!(
                    "report output must not contain an input: {}",
                    input.display()
                );
            }
        }
    }
    if output.is_file() {
        bail!(
            "report output must be a directory, not a regular file: {}",
            output.display()
        );
    }
    if output.exists() && !force {
        bail!(
            "report output already exists: {}; pass --force to replace it",
            output.display()
        );
    }
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("report output has no parent: {}", output.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create report parent: {}", parent.display()))?;
    let stage = tempfile::Builder::new()
        .prefix(".clab-report-")
        .tempdir_in(parent)?;
    for (name, contents) in files {
        let relative = Path::new(name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            bail!("report file name must be a relative path without traversal: {name}");
        }
        let path = stage.path().join(relative);
        let parent = path.parent().expect("relative report file has a parent");
        fs::create_dir_all(parent)?;
        let mut file = fs::File::create(&path)?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    let staged = stage.keep();
    let backup = parent.join(format!(".clab-report-backup-{}", std::process::id()));
    if output.exists() {
        fs::rename(&output, &backup)?;
    }
    if let Err(error) = fs::rename(&staged, &output) {
        if backup.exists() {
            let _ = fs::rename(&backup, &output);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_dir_all(backup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::absolute_path;
    use std::path::Path;

    #[test]
    fn absolute_path_drops_current_directory_components() {
        assert_eq!(
            absolute_path(Path::new("/data/./run/../input/./a.toml")).unwrap(),
            Path::new("/data/input/a.toml")
        );
        let relative = absolute_path(Path::new("./ford/./config.toml")).unwrap();
        assert!(relative.is_absolute());
        assert!(!relative.to_string_lossy().contains("/./"));
        assert!(relative.ends_with("ford/config.toml"));
    }
}
