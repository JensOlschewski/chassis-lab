use std::{
    fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use clab_core::svc_reference::load_svc_reference;
use clap::Args;
use sha2::{Digest, Sha256};

use crate::analysis::{svc, vertical};

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
                "failed to load config/run '{}': {error}",
                args.input.display()
            )
        })?;
    let has_svc = !config.simulation_json.as_os_str().is_empty();
    let vertical_path = config
        .vertical
        .as_ref()
        .map(|vertical| match vertical.source_kind {
            clab_core::config::SourceKind::Simulation => &vertical.simulation_hdf5,
            clab_core::config::SourceKind::Measurement => &vertical.measurement_hdf5,
        });
    match (has_svc, vertical_path) {
        (true, Some(vertical_path)) => write_combined_run(&config, vertical_path, args),
        (true, None) if is_hdf5(&config.simulation_json) => {
            if config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement) {
                bail!(
                    "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
                    config.simulation_json.display()
                );
            }
            write_svc_hdf5_run(&config, args, &args.input)
        }
        (true, None) => match config.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) => {
                write_svc(&config, args, Some(&args.input))
            }
            Some(clab_core::config::SourceKind::Measurement) => {
                let reference = load_svc_reference(&config.simulation_json)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                write_svc_reference(&reference, args, &config, Some(&args.input))
            }
            None => unreachable!(),
        },
        (false, Some(vertical_path)) => {
            write_vertical(&config, vertical_path, args, Some(&args.input))
        }
        (false, None) => Ok(()),
    }
}

fn validate_input_file(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("input file cannot be found: {}", path.display());
    }
    Ok(())
}

fn write_combined_run(
    config: &clab_core::config::CaseConfig,
    vertical_path: &Path,
    args: &AnalyzeArgs,
) -> Result<()> {
    let (svc_report, svc_detail, svc_csv, svc_metrics, svc_warnings) = if is_hdf5(
        &config.simulation_json,
    ) {
        if config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement) {
            bail!(
                "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
                config.simulation_json.display()
            );
        }
        let report = svc::analyze_hdf5(config)?;
        (
            report.report,
            report.detail,
            report.csv,
            report.metric_count,
            report.warnings,
        )
    } else {
        match config.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) => {
                let report = svc::analyze(config)?;
                (
                    report.report,
                    report.detail,
                    report.csv,
                    report.metric_count,
                    report.warnings,
                )
            }
            Some(clab_core::config::SourceKind::Measurement) => {
                let reference = load_svc_reference(&config.simulation_json)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                let reports = svc_reference_reports(&reference, &args.config, config)?;
                let metrics = reports.report["metrics"].as_array().unwrap().len();
                (
                    reports.report,
                    reports.detail,
                    reports.csv,
                    metrics,
                    reports.warnings,
                )
            }
            None => unreachable!(),
        }
    };
    let vertical_report = vertical::analyze_input(config, vertical_path, args.output.is_some())?;
    println!("SVC analysis: {svc_metrics} metrics");
    let mut warnings = svc_warnings;
    warnings.extend(vertical_report.warnings.iter().cloned());
    warnings.sort();
    warnings.dedup();
    print_warnings(&warnings, args);
    print_svc_metrics(svc_report["metrics"].as_array().unwrap());
    let Some(output) = &args.output else {
        return Ok(());
    };
    let bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "case": {"name": config.name, "loadcase": config.loadcase},
        "inputs": {"config": provenance(&args.config)?, "run": provenance(&args.input)?},
        "domains": [
            {"domain": "svc", "metric_count": svc_metrics},
            {"domain": "knc", "test": "vertical", "metric_count": 0, "curve_count": vertical_report.curve_count}
        ],
        "metrics": svc_report["metrics"],
        "warnings": warnings,
        "normalization": vertical_report.normalization,
    });
    let mut svc_detail: serde_json::Value = serde_json::from_str(&svc_detail)?;
    svc_detail["case"] = bundle["case"].clone();
    svc_detail["inputs"]["run"] = bundle["inputs"]["run"].clone();
    let mut vertical_detail: serde_json::Value = serde_json::from_str(&vertical_report.detail)?;
    vertical_detail["case"] = bundle["case"].clone();
    let mut vertical_inputs = bundle["inputs"].clone();
    vertical_inputs["input"] = provenance(vertical_path)?;
    if let Some(static_input) = config
        .vertical
        .as_ref()
        .and_then(|vertical| vertical.static_simulation_hdf5.as_ref())
    {
        vertical_inputs["static_simulation"] = provenance(static_input)?;
    }
    vertical_detail["inputs"] = vertical_inputs;
    let mut files = vec![
        ("report.json", serde_json::to_string_pretty(&bundle)? + "\n"),
        ("metrics.csv", svc_csv),
        (
            "svc.json",
            serde_json::to_string_pretty(&svc_detail)? + "\n",
        ),
        (
            "knc-vertical.json",
            serde_json::to_string_pretty(&vertical_detail)? + "\n",
        ),
    ];
    files.extend(
        vertical_report
            .plots
            .iter()
            .map(|(name, contents)| (name.as_str(), contents.clone())),
    );
    let mut inputs = vec![
        args.config.as_path(),
        args.input.as_path(),
        config.simulation_json.as_path(),
        vertical_path,
    ];
    add_vertical_inputs(&mut inputs, config);
    write_bundle(output, &files, args.force, &inputs)?;
    println!("report: {}", output.display());
    Ok(())
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
            let config =
                clab_core::config::load_config_with_input(&args.config, &args.input, false)
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "failed to load analysis config '{}': {error}",
                            args.config.display()
                        )
                    })?;
            let reference = load_svc_reference(&args.input)
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            write_svc_reference(&reference, args, &config, None)
        }
        Some("simulation") => {
            let config =
                clab_core::config::load_config_with_input(&args.config, &args.input, false)
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
    if input.summary().source_kind == SourceKind::Measurement
        && let Ok(config) =
            clab_core::config::load_config_with_input(&args.config, &args.input, false)
        && config.svc_source_kind == Some(clab_core::config::SourceKind::Measurement)
    {
        bail!(
            "SVC analysis does not support measurement HDF5 input '{}'; provide an SVC measurement JSON reference or a simulation HDF5 input",
            args.input.display()
        );
    }
    if input.summary().source_kind == SourceKind::Simulation
        && let Ok(config) =
            clab_core::config::load_config_with_input(&args.config, &args.input, false)
        && svc::supports_hdf5(&config, &input)
    {
        return write_svc_hdf5(&config, args);
    }
    analyze_vertical(args)
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
    println!(
        "SVC reference analysis: {} metrics",
        reports.report["metrics"].as_array().unwrap().len()
    );
    print_warnings(&reports.warnings, args);
    print_svc_metrics(reports.report["metrics"].as_array().unwrap());
    if let Some(output) = &args.output {
        write_bundle(
            output,
            &[
                ("report.json", reports.detail.clone()),
                ("metrics.csv", reports.csv),
                ("svc.json", reports.detail),
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
                    "SVC metric '{}' is missing from measurement reference",
                    metric.name
                ));
            }
            serde_json::json!({
                "domain": "svc", "test": null, "metric": metric.name, "unit": metric.unit, "value": value,
            })
        })
        .collect::<Vec<_>>();
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
    println!("SVC analysis: {} metrics", report.metric_count);
    print_warnings(&report.warnings, args);
    print_svc_metrics(report.report["metrics"].as_array().unwrap());
    if let Some(output) = &args.output {
        let mut bundle = report.report;
        if let Some(run) = run {
            bundle["inputs"]["run"] = provenance(run)?;
        }
        let detail = serde_json::to_string_pretty(&bundle)? + "\n";
        write_bundle(
            output,
            &[
                ("report.json", detail.clone()),
                ("metrics.csv", report.csv),
                ("svc.json", detail),
            ],
            args.force,
            &[&args.config, &args.input, &config.simulation_json],
        )?;
        println!("report: {}", output.display());
    }
    Ok(())
}

fn print_svc_metrics(metrics: &[serde_json::Value]) {
    for metric in metrics {
        let value = if metric["value"].is_null() {
            if std::io::stdout().is_terminal() {
                "\x1b[38;5;208mmissing\x1b[0m".into()
            } else {
                "missing".into()
            }
        } else {
            metric["value"].to_string()
        };
        println!(
            "  {} = {} {}",
            metric["metric"].as_str().unwrap(),
            value,
            metric["unit"].as_str().unwrap()
        );
    }
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

fn analyze_vertical(args: &AnalyzeArgs) -> Result<()> {
    let config = clab_core::config::load_config_with_input(&args.config, &args.input, true)
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to load analysis config '{}': {error}",
                args.config.display()
            )
        })?;
    write_vertical(&config, &args.input, args, None)
}

fn write_vertical(
    config: &clab_core::config::CaseConfig,
    input: &Path,
    args: &AnalyzeArgs,
    run: Option<&Path>,
) -> Result<()> {
    let report = vertical::analyze_input(config, input, args.output.is_some())?;
    print_warnings(&report.warnings, args);
    let Some(output) = &args.output else {
        return Ok(());
    };
    let mut bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "case": {"name": config.name, "loadcase": config.loadcase},
        "inputs": {"config": provenance(&args.config)?, "input": provenance(input)?},
        "domains": [{"domain": "knc", "test": "vertical", "metric_count": 0, "curve_count": report.curve_count}],
        "warnings": report.warnings,
        "normalization": report.normalization,
    });
    if let Some(run) = run {
        bundle["inputs"]["run"] = provenance(run)?;
    }
    let mut detail: serde_json::Value = serde_json::from_str(&report.detail)?;
    detail["case"] = bundle["case"].clone();
    detail["inputs"] = bundle["inputs"].clone();
    let detail = serde_json::to_string_pretty(&detail)? + "\n";
    let mut files = vec![
        ("report.json", serde_json::to_string_pretty(&bundle)? + "\n"),
        ("knc-vertical.json", detail),
    ];
    files.extend(
        report
            .plots
            .iter()
            .map(|(name, contents)| (name.as_str(), contents.clone())),
    );
    let mut inputs = vec![args.config.as_path(), input];
    add_vertical_inputs(&mut inputs, config);
    if let Some(run) = run {
        inputs.push(run);
    }
    write_bundle(output, &files, args.force, &inputs)?;
    println!("report: {}", output.display());
    Ok(())
}

fn add_vertical_inputs<'a>(inputs: &mut Vec<&'a Path>, config: &'a clab_core::config::CaseConfig) {
    let Some(vertical) = &config.vertical else {
        return;
    };
    inputs.push(&vertical.simulation_hdf5);
    inputs.push(&vertical.measurement_hdf5);
    if let Some(static_simulation) = &vertical.static_simulation_hdf5 {
        inputs.push(static_simulation);
    }
}

fn provenance(path: &Path) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"path": path, "sha256": format!("{:x}", Sha256::digest(fs::read(path)?))}),
    )
}

pub(crate) fn write_bundle(
    output: &Path,
    files: &[(&str, String)],
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
        file.write_all(contents.as_bytes())?;
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
