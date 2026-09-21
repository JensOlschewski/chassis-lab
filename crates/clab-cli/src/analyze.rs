use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clab_core::input_hdf5::{Hdf5Input, open_hdf5_input};
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
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Replace report files already in output directory.
    #[arg(long)]
    force: bool,
    /// Print every calculated metric.
    #[arg(long)]
    verbose: bool,
}

pub(crate) fn analyze(args: AnalyzeArgs) -> Result<()> {
    let extension = args
        .input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("toml") => analyze_run(&args),
        Some("json") => analyze_svc(&args),
        Some("h5" | "hdf5") => analyze_hdf5(&args),
        _ => bail!("analyze accepts run TOML, SVC JSON, or normalized HDF5 input"),
    }
}

fn analyze_run(args: &AnalyzeArgs) -> Result<()> {
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
        (true, None) => match config.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) => {
                write_svc(&config, args, Some(&args.input))
            }
            Some(clab_core::config::SourceKind::Measurement) => {
                let reference = load_svc_reference(&config.simulation_json)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                write_svc_reference(&reference, args)
            }
            None => unreachable!(),
        },
        (false, Some(vertical_path)) => {
            write_vertical(&config, vertical_path, args, Some(&args.input))
        }
        (false, None) => Ok(()),
    }
}

fn write_combined_run(
    config: &clab_core::config::CaseConfig,
    vertical_path: &Path,
    args: &AnalyzeArgs,
) -> Result<()> {
    let (svc_report, svc_detail, svc_csv, svc_metrics, svc_warnings) = match config.svc_source_kind
    {
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
            let (report, detail, csv) = svc_reference_reports(&reference, &args.config)?;
            let metrics = report["metrics"].as_array().unwrap().len();
            (report, detail, csv, metrics, Vec::new())
        }
        None => unreachable!(),
    };
    let vertical_report = vertical::analyze_input(config, vertical_path)?;
    println!("SVC analysis: {svc_metrics} metrics");
    let mut warnings = svc_warnings;
    warnings.extend(vertical_report.warnings.iter().cloned());
    warnings.sort();
    warnings.dedup();
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    if args.verbose {
        for metric in svc_report["metrics"].as_array().unwrap() {
            println!(
                "  {} = {} {}",
                metric["metric"].as_str().unwrap(),
                metric["value"],
                metric["unit"].as_str().unwrap()
            );
        }
    }
    let Some(output) = &args.output else {
        return Ok(());
    };
    let bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "inputs": {"config": provenance(&args.config)?, "run": provenance(&args.input)?},
        "domains": [
            {"domain": "svc", "metric_count": svc_metrics},
            {"domain": "knc", "test": "vertical", "metric_count": 0, "curve_count": vertical_report.curve_count}
        ],
        "metrics": svc_report["metrics"],
        "warnings": warnings,
        "normalization": vertical_report.normalization,
    });
    let mut files = vec![
        ("report.json", serde_json::to_string_pretty(&bundle)? + "\n"),
        ("metrics.csv", svc_csv),
        ("svc.json", svc_detail),
        ("knc-vertical.json", vertical_report.detail),
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

fn analyze_svc(args: &AnalyzeArgs) -> Result<()> {
    if let Ok(reference) = load_svc_reference(&args.input) {
        return write_svc_reference(&reference, args);
    }
    let config = clab_core::config::load_config_with_input(&args.config, &args.input, false)
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to load analysis config '{}': {error}",
                args.config.display()
            )
        })?;
    write_svc(&config, args, None)
}

fn analyze_hdf5(args: &AnalyzeArgs) -> Result<()> {
    let input = open_hdf5_input(&args.input)
        .with_context(|| format!("failed to read HDF5 '{}'", args.input.display()))?;
    if let Hdf5Input::Simulation(simulation) = input
        && let Ok(config) =
            clab_core::config::load_config_with_input(&args.config, &args.input, false)
        && svc::supports_hdf5(&config, &simulation)
    {
        return write_svc_hdf5(&config, args);
    }
    analyze_vertical(args)
}

fn write_svc_reference(
    reference: &clab_core::svc_reference::SvcReference,
    args: &AnalyzeArgs,
) -> Result<()> {
    let (report, detail, csv) = svc_reference_reports(reference, &args.config)?;
    println!(
        "SVC reference analysis: {} metrics",
        report["metrics"].as_array().unwrap().len()
    );
    if let Some(output) = &args.output {
        write_bundle(
            output,
            &[
                ("report.json", detail.clone()),
                ("metrics.csv", csv),
                ("svc.json", detail),
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
) -> Result<(serde_json::Value, String, String)> {
    let metrics = reference.values.iter().map(|(name, value)| serde_json::json!({
        "domain": "svc", "test": null, "metric": name, "unit": svc_unit(name), "value": value,
    })).collect::<Vec<_>>();
    let report = serde_json::json!({
        "schema_version": 1, "command": "analyze", "inputs": {"config": provenance(config)?, "reference": provenance(&reference.path)?}, "metrics": metrics,
    });
    let mut csv = csv::Writer::from_writer(Vec::new());
    csv.write_record(["run", "domain", "test", "metric", "unit", "value"])?;
    for metric in report["metrics"].as_array().unwrap() {
        csv.write_record([
            "reference",
            "svc",
            "",
            metric["metric"].as_str().unwrap(),
            metric["unit"].as_str().unwrap(),
            &metric["value"].to_string(),
        ])?;
    }
    Ok((
        report.clone(),
        serde_json::to_string_pretty(&report)? + "\n",
        String::from_utf8(csv.into_inner()?)?,
    ))
}

fn svc_unit(name: &str) -> &'static str {
    if name.ends_with("_percent") {
        "%"
    } else if name.ends_with("_kg") {
        "kg"
    } else if name.ends_with("_mm") {
        "mm"
    } else {
        ""
    }
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

fn write_svc_report(
    config: &clab_core::config::CaseConfig,
    args: &AnalyzeArgs,
    run: Option<&Path>,
    report: svc::Reports,
) -> Result<()> {
    println!("SVC analysis: {} metrics", report.metric_count);
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    if args.verbose {
        for metric in report.report["metrics"].as_array().unwrap() {
            println!(
                "  {} = {} {}",
                metric["metric"].as_str().unwrap(),
                metric["value"],
                metric["unit"].as_str().unwrap()
            );
        }
    }
    if let Some(output) = &args.output {
        let mut bundle = report.report;
        if let Some(run) = run {
            bundle["inputs"]["run"] = provenance(run)?;
        }
        write_bundle(
            output,
            &[
                ("report.json", serde_json::to_string_pretty(&bundle)? + "\n"),
                ("metrics.csv", report.csv),
                ("svc.json", report.detail),
            ],
            args.force,
            &[&args.config, &args.input, &config.simulation_json],
        )?;
        println!("report: {}", output.display());
    }
    Ok(())
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
    let report = vertical::analyze_input(config, input)?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    let Some(output) = &args.output else {
        return Ok(());
    };
    let bundle = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "inputs": {"config": provenance(&args.config)?, "input": provenance(input)?},
        "domains": [{"domain": "knc", "test": "vertical", "metric_count": 0, "curve_count": report.curve_count}],
        "warnings": report.warnings,
        "normalization": report.normalization,
    });
    let mut files = vec![
        ("report.json", serde_json::to_string_pretty(&bundle)? + "\n"),
        ("knc-vertical.json", report.detail),
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
