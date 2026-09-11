use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clab_core::config::{CaseConfig, Domain};
use clap::{Args, ValueEnum};
use thiserror::Error;

pub(crate) mod svc;
pub(crate) mod vertical;

#[derive(Debug, Error)]
#[error("validation tolerance failed")]
pub(crate) struct ToleranceFailure;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum DomainArg {
    Svc,
    Knc,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum TestArg {
    Vertical,
}

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    /// TOML case configuration.
    config: PathBuf,
    /// Domain to validate; repeat to select multiple domains.
    #[arg(short = 'd', long = "domain")]
    domains: Vec<DomainArg>,
    /// Named KnC test to validate; currently only vertical.
    #[arg(short = 't', long = "test")]
    tests: Vec<TestArg>,
    /// Directory for deterministic reports.
    #[arg(long)]
    output_dir: Option<PathBuf>,
    #[arg(long)]
    json_output: Option<PathBuf>,
    #[arg(long)]
    csv_output: Option<PathBuf>,
    #[arg(long)]
    plot_output: Option<PathBuf>,
    #[arg(short, long)]
    force: bool,
    #[arg(long, value_enum, default_value_t = svc::Color::Auto)]
    color: svc::Color,
}

pub(crate) fn validate(args: ValidateArgs) -> Result<()> {
    let config = clab_core::config::load_config(&args.config).map_err(|e| {
        anyhow::anyhow!(
            "failed to load configuration '{}': {e}",
            args.config.display()
        )
    })?;
    let selected = selection(&config, &args.domains, &args.tests)?;
    let reports = report_paths(&args, &selected)?;
    validate_outputs(&reports, args.force, protected_inputs(&config)?)?;

    let mut contents = Vec::new();
    let mut tolerance_failed = false;
    for domain in selected {
        match domain {
            Domain::Svc => {
                let (report, failed) = svc::validate(&config, args.color)?;
                if let Some(paths) = &reports.svc {
                    contents.extend([
                        (paths.0.clone(), report.json),
                        (paths.1.clone(), report.csv),
                    ]);
                }
                tolerance_failed |= failed;
            }
            Domain::Knc => {
                let report = vertical::validate(&config)?;
                if let Some(paths) = &reports.knc {
                    contents.extend([
                        (paths.0.clone(), report.json),
                        (paths.1.clone(), report.csv),
                        (paths.2.clone(), report.svg),
                    ]);
                }
            }
        }
    }
    publish_staged(stage_reports(&contents, args.force)?, args.force)?;
    if tolerance_failed {
        Err(ToleranceFailure.into())
    } else {
        Ok(())
    }
}

fn selection(config: &CaseConfig, domains: &[DomainArg], tests: &[TestArg]) -> Result<Vec<Domain>> {
    let configured = config.configured_domains();
    let mut selected = if tests.contains(&TestArg::Vertical) && domains.is_empty() {
        vec![Domain::Knc]
    } else if domains.is_empty() {
        configured.clone()
    } else {
        domains
            .iter()
            .map(|domain| match domain {
                DomainArg::Svc => Domain::Svc,
                DomainArg::Knc => Domain::Knc,
            })
            .collect::<Vec<_>>()
    };
    if tests.contains(&TestArg::Vertical) && !selected.contains(&Domain::Knc) {
        bail!("-t vertical requires selecting -d knc");
    }
    selected.sort_by_key(|domain| matches!(domain, Domain::Knc));
    selected.dedup();
    for domain in &selected {
        if !configured.contains(domain) {
            bail!(
                "selected domain is not configured: {}",
                match domain {
                    Domain::Svc => "svc",
                    Domain::Knc => "knc",
                }
            );
        }
    }
    if selected.is_empty() {
        bail!("configuration has no validation domains");
    }
    Ok(selected)
}

struct ReportPaths {
    svc: Option<(PathBuf, PathBuf)>,
    knc: Option<(PathBuf, PathBuf, PathBuf)>,
}
fn report_paths(args: &ValidateArgs, selected: &[Domain]) -> Result<ReportPaths> {
    let individual =
        args.json_output.is_some() || args.csv_output.is_some() || args.plot_output.is_some();
    if args.output_dir.is_some() && individual {
        bail!("individual report options cannot be combined with --output-dir");
    }
    if individual && selected.len() != 1 {
        bail!("individual report options require exactly one selected domain");
    }
    if args.plot_output.is_some() && !selected.contains(&Domain::Knc) {
        bail!("--plot-output requires selecting KnC");
    }
    if args.output_dir.is_none() && !individual {
        return Ok(ReportPaths {
            svc: None,
            knc: None,
        });
    }
    if let Some(plot) = &args.plot_output
        && plot
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("svg"))
    {
        bail!("vertical plot output must use .svg extension");
    }
    if let Some(directory) = &args.output_dir {
        fs::create_dir_all(directory).with_context(|| {
            format!("failed to create output directory: {}", directory.display())
        })?;
        return Ok(ReportPaths {
            svc: selected
                .contains(&Domain::Svc)
                .then(|| (directory.join("svc.json"), directory.join("svc.csv"))),
            knc: selected.contains(&Domain::Knc).then(|| {
                (
                    directory.join("knc-vertical.json"),
                    directory.join("knc-vertical.csv"),
                    directory.join("knc-vertical.svg"),
                )
            }),
        });
    }
    if selected.len() > 1 {
        return Ok(ReportPaths {
            svc: selected
                .contains(&Domain::Svc)
                .then(|| (PathBuf::from("svc.json"), PathBuf::from("svc.csv"))),
            knc: selected.contains(&Domain::Knc).then(|| {
                (
                    PathBuf::from("knc-vertical.json"),
                    PathBuf::from("knc-vertical.csv"),
                    PathBuf::from("knc-vertical.svg"),
                )
            }),
        });
    }
    match selected {
        [Domain::Svc] => Ok(ReportPaths { svc: match (args.json_output.clone(), args.csv_output.clone()) { (None, None) => None, (json, csv) => Some((json.ok_or_else(|| anyhow::anyhow!("--json-output and --csv-output must be provided together"))?, csv.ok_or_else(|| anyhow::anyhow!("--json-output and --csv-output must be provided together"))?)) }, knc: None }),
        [Domain::Knc] => Ok(ReportPaths { svc: None, knc: match (args.json_output.clone(), args.csv_output.clone(), args.plot_output.clone()) { (None, None, None) => None, (json, csv, svg) => Some((json.ok_or_else(|| anyhow::anyhow!("vertical reports require --json-output, --csv-output, and --plot-output"))?, csv.ok_or_else(|| anyhow::anyhow!("vertical reports require --json-output, --csv-output, and --plot-output"))?, svg.ok_or_else(|| anyhow::anyhow!("vertical reports require --json-output, --csv-output, and --plot-output"))?)) } }),
        _ => unreachable!(),
    }
}

fn protected_inputs(config: &CaseConfig) -> Result<Vec<PathBuf>> {
    let mut paths = vec![config.path.clone()];
    if config.configured_domains().contains(&Domain::Svc) {
        paths.push(config.simulation_json.clone());
        if let Some(validation) = &config.validation {
            paths.push(validation.reference.clone());
        }
        if let Some(path) = &config.svc_signal_map {
            paths.push(path.clone());
        }
    }
    if let Some(vertical) = &config.vertical {
        paths.extend([
            vertical.measurement_hdf5.clone(),
            vertical.simulation_hdf5.clone(),
        ]);
        if let Some(path) = &vertical.signal_map {
            paths.push(path.clone());
        }
    }
    Ok(paths)
}

fn validate_outputs(reports: &ReportPaths, force: bool, inputs: Vec<PathBuf>) -> Result<()> {
    let paths: Vec<&Path> = reports
        .svc
        .iter()
        .flat_map(|p| [&p.0, &p.1])
        .chain(reports.knc.iter().flat_map(|p| [&p.0, &p.1, &p.2]))
        .map(PathBuf::as_path)
        .collect();
    let inputs: Result<Vec<_>> = inputs
        .iter()
        .map(|path| path.canonicalize().map_err(Into::into))
        .collect();
    let inputs = inputs?;
    let mut unique = std::collections::BTreeSet::new();
    for path in paths {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output parent directory: {}",
                parent.display()
            )
        })?;
        let parent = parent.canonicalize()?;
        if path.is_dir() {
            bail!("report output path is a directory: {}", path.display());
        }
        let canonical = if path.exists() {
            path.canonicalize()?
        } else {
            parent.join(path.file_name().unwrap())
        };
        if !unique.insert(canonical.clone()) {
            bail!("report output paths must be different");
        }
        if inputs.contains(&canonical) {
            bail!(
                "report output must not replace an input: {}",
                path.display()
            );
        }
        if path.exists() && !force {
            bail!(
                "report output already exists: {}; pass --force to replace it",
                path.display()
            );
        }
    }
    Ok(())
}

struct StagedReport {
    path: PathBuf,
    temporary: tempfile::NamedTempFile,
    backup: Option<tempfile::NamedTempFile>,
}

fn stage_reports(reports: &[(PathBuf, String)], force: bool) -> Result<Vec<StagedReport>> {
    reports
        .iter()
        .map(|(path, content)| {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(content.as_bytes())?;
            temporary.flush()?;
            let backup = if force && path.exists() {
                let backup = tempfile::NamedTempFile::new_in(parent)?;
                fs::copy(path, backup.path())?;
                Some(backup)
            } else {
                None
            };
            Ok(StagedReport {
                path: path.clone(),
                temporary,
                backup,
            })
        })
        .collect()
}
fn publish_staged(staged: Vec<StagedReport>, force: bool) -> Result<()> {
    let mut published = Vec::new();
    for report in staged {
        let path = report.path;
        let result = if force {
            report.temporary.persist(&path)
        } else {
            report.temporary.persist_noclobber(&path)
        };
        match result {
            Ok(_) => published.push((path, report.backup)),
            Err(error) => {
                let rollback = rollback_published(published);
                if let Err(rollback) = rollback {
                    return Err(error.error).context(format!(
                        "report publication failed and rollback also failed: {rollback:#}"
                    ));
                }
                return Err(error.error.into());
            }
        }
    }
    Ok(())
}

fn rollback_published(published: Vec<(PathBuf, Option<tempfile::NamedTempFile>)>) -> Result<()> {
    for (path, backup) in published.into_iter().rev() {
        fs::remove_file(&path)?;
        if let Some(backup) = backup {
            backup.persist(&path).map_err(|error| error.error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Domain, ValidateArgs, publish_staged, report_paths, stage_reports};
    use std::fs;

    #[test]
    fn failed_force_publish_restores_prior_outputs() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.json");
        let second = directory.path().join("second.csv");
        fs::write(&first, "old").unwrap();
        let staged = stage_reports(
            &[
                (first.clone(), "new".into()),
                (second.clone(), "new".into()),
            ],
            true,
        )
        .unwrap();
        fs::create_dir(&second).unwrap();
        assert!(publish_staged(staged, true).is_err());
        assert_eq!(fs::read_to_string(first).unwrap(), "old");
        assert!(second.is_dir());
    }

    #[test]
    fn combined_validation_without_report_options_writes_no_reports() {
        let args = ValidateArgs {
            config: "config.toml".into(),
            domains: Vec::new(),
            tests: Vec::new(),
            output_dir: None,
            json_output: None,
            csv_output: None,
            plot_output: None,
            force: false,
            color: super::svc::Color::Auto,
        };
        let reports = report_paths(&args, &[Domain::Svc, Domain::Knc]).unwrap();

        assert!(reports.svc.is_none());
        assert!(reports.knc.is_none());
    }
}
