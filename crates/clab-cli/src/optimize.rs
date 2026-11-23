//! `clab optimize`: evaluates or optimizes a design against a problem file.
//!
//! The optimization runs inside the compiled MATLAB solver (`mks_solver
//! --optimize`); this side validates the problem, starts the solver and reports.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clab_core::config::problem::Problem;
use clap::Args;
use serde_json::Value;

use crate::convert;
use crate::matlab::{self, file_path};

#[derive(Debug, Args)]
pub(crate) struct OptimizeArgs {
    /// Optimization problem TOML.
    problem: PathBuf,
    /// Start design JSON. Also sets the objective scales that are not given.
    /// Without it, the start is the midpoint of the bounds.
    #[arg(long, value_name = "FILE")]
    design: Option<PathBuf>,
    /// Write the optimized design to PATH: a file, or a directory that receives
    /// `<problem>-optimized.json`. Without PATH, the file goes to the current
    /// directory. Give the problem before this flag.
    #[arg(long, value_name = "PATH", num_args = 0..=1, conflicts_with = "dry_run")]
    json: Option<Option<PathBuf>>,
    /// Evaluate the start design and stop.
    #[arg(long)]
    dry_run: bool,
    /// Replace an existing --json file.
    #[arg(long, requires = "json")]
    force: bool,
    /// Show optimizer progress on stderr.
    #[arg(long)]
    progress: bool,
    /// Proceed without prompting when --design is not given.
    #[arg(short, long)]
    yes: bool,
}

pub(crate) fn optimize(args: OptimizeArgs) -> Result<()> {
    let problem_file = file_path(&args.problem, "problem")?;
    let given_design = args
        .design
        .as_deref()
        .map(|path| file_path(path, "design"))
        .transpose()?;
    let problem = Problem::load(&problem_file)?;
    let output = args
        .json
        .as_ref()
        .map(|path| matlab::output_path(path.as_deref(), &args.problem, "optimized"))
        .transpose()?;
    if let Some(output) = &output {
        let mut inputs = vec![(problem_file.as_path(), "problem")];
        if let Some(design) = &given_design {
            inputs.push((design.as_path(), "design"));
        }
        matlab::check_output(output, &inputs, args.force)?;
    }

    let midpoint_file;
    let design = match given_design {
        Some(design) => design,
        None => {
            midpoint_file = midpoint_design(&problem, args.yes)?;
            midpoint_file.path().to_path_buf()
        }
    };

    let mut problem_json = scratch_json()?;
    serde_json::to_writer(&mut problem_json, &problem.solver_json())?;
    let result_file = matlab::scratch_file(output.as_deref())?;

    let mut solver_args = vec![
        Path::new("--optimize"),
        problem_json.path(),
        result_file.path(),
        Path::new("--design"),
        design.as_path(),
    ];
    if args.dry_run {
        solver_args.push(Path::new("--dry-run"));
    }
    matlab::run_solver(
        &solver_args,
        problem_file.parent().unwrap_or(Path::new(".")),
        args.progress,
    )?;

    let result: Value = serde_json::from_slice(&std::fs::read(result_file.path())?)
        .context("solver wrote invalid JSON")?;
    print!("{}", report(&result, &problem, args.dry_run)?);

    if let Some(output) = &output {
        let mut text = String::new();
        matlab::format_json(&result["design"], 0, &mut text);
        text.push('\n');
        std::fs::write(result_file.path(), text)
            .with_context(|| format!("cannot write {}", output.display()))?;
        result_file
            .persist(output)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("{}", output.display());
    }
    Ok(())
}

fn scratch_json() -> Result<tempfile::NamedTempFile> {
    tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .context("cannot create a scratch file")
}

/// A start design at the midpoint of each bound, after the user confirms it.
fn midpoint_design(problem: &Problem, yes: bool) -> Result<tempfile::NamedTempFile> {
    let start: serde_json::Map<String, Value> = problem
        .design
        .iter()
        .map(|d| (d.name.clone(), Value::from((d.lower + d.upper) / 2.0)))
        .collect();
    let values: Vec<String> = start
        .iter()
        .map(|(name, v)| format!("{name} = {v}"))
        .collect();
    convert::confirm(
        vec![format!(
            "no --design given; starting from the midpoint of the bounds: {}",
            values.join(", ")
        )],
        yes,
    )?;
    let mut file = scratch_json()?;
    serde_json::to_writer(&mut file, &start)?;
    Ok(file)
}

/// MATLAB's `jsonencode` writes a one-element struct array as an object and an
/// empty one as `[]`.
fn items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![value],
        _ => Vec::new(),
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("?")
}

fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|col| {
            rows.iter()
                .map(|row| row[col].chars().count())
                .chain([header[col].chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    for row in std::iter::once(header.iter().map(|s| s.to_string()).collect()).chain(rows.to_vec())
    {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        writeln!(out, "  {}", cells.join("  ").trim_end()).unwrap();
    }
    out
}

fn report(result: &Value, problem: &Problem, dry_run: bool) -> Result<String> {
    let mut out = String::new();

    let rows: Vec<Vec<String>> = problem
        .design
        .iter()
        .map(|d| {
            vec![
                d.name.clone(),
                format!("{:.6}", number(&result["start_design"][&d.name])),
                format!("{:.6}", number(&result["design"][&d.name])),
                format!("[{}, {}]", d.lower, d.upper),
            ]
        })
        .collect();
    let final_column = if dry_run { "design" } else { "optimized" };
    writeln!(out, "design")?;
    out += &table(&["variable", "start", final_column, "bounds"], &rows);

    let rows: Vec<Vec<String>> = items(&result["objectives"])
        .iter()
        .map(|o| {
            vec![
                text(&o["name"]).to_owned(),
                format!("{}", number(&o["weight"])),
                format!("{:.6}", number(&o["scale"])),
                format!("{:.6}", number(&o["start"])),
                format!("{:.6}", number(&o["value"])),
                format!("{:.4}", number(&o["loss"])),
            ]
        })
        .collect();
    writeln!(out, "\nobjectives (minimized)")?;
    out += &table(
        &["name", "weight", "scale", "start", "value", "value/scale"],
        &rows,
    );

    let constraints = items(&result["constraints"]);
    if !constraints.is_empty() {
        let rows: Vec<Vec<String>> = constraints
            .iter()
            .map(|c| {
                vec![
                    text(&c["name"]).to_owned(),
                    format!("{:.6}", number(&c["value"])),
                    text(&c["relation"]).to_owned(),
                    format!("{:.6}", number(&c["limit"])),
                    if c["ok"] == true { "ok" } else { "VIOLATED" }.to_owned(),
                ]
            })
            .collect();
        writeln!(out, "\nconstraints")?;
        out += &table(&["name", "value", "", "limit", "status"], &rows);
    }

    writeln!(out)?;
    if dry_run {
        writeln!(out, "J {:.4} (start design)", number(&result["J"]))?;
    } else {
        writeln!(
            out,
            "J {:.4} -> {:.4}",
            number(&result["start_J"]),
            number(&result["J"])
        )?;
        let f = &result["fmincon"];
        writeln!(
            out,
            "fmincon exit {}, {} iterations, {} evaluations ({} failed)",
            number(&f["exitflag"]),
            number(&f["iterations"]),
            number(&f["evaluations"]),
            number(&f["failed_evaluations"])
        )?;
        if number(&f["failed_evaluations"]) > 0.0 {
            writeln!(out, "last evaluation error: {}", text(&f["last_error"]))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem() -> Problem {
        Problem::parse(
            r#"
[models]
m = "m.json"
[design]
A = [0, 10]
[objectives.o]
value = "m.characteristics.x"
"#,
            Path::new("/base"),
        )
        .unwrap()
    }

    #[test]
    fn reports_single_objective_object_and_no_constraints() {
        let result: Value = serde_json::json!({
            "design": {"A": 5.0}, "start_design": {"A": 1.0},
            "objectives": {"name": "o", "weight": 1, "scale": 3.0, "start": 3.0,
                "value": 2.0, "loss": 0.6667},
            "constraints": [],
            "J": 0.6667, "start_J": 1.0,
            "fmincon": {"exitflag": 1, "iterations": 4, "evaluations": 12, "failed_evaluations": 0, "last_error": ""},
        });
        let text = report(&result, &problem(), false).unwrap();
        assert!(
            text.contains("A         1.000000  5.000000   [0, 10]"),
            "{text}"
        );
        assert!(
            text.contains("o     1       3.000000  3.000000  2.000000  0.6667"),
            "{text}"
        );
        assert!(!text.contains("constraints"));
        assert!(text.contains("J 1.0000 -> 0.6667"));
        assert!(text.contains("12 evaluations (0 failed)"));
    }

    #[test]
    fn flags_violated_constraints() {
        let result: Value = serde_json::json!({
            "design": {"A": 1.0}, "start_design": {"A": 1.0},
            "objectives": [], "J": 1.0,
            "constraints": {"name": "c", "value": 2.0, "relation": "<=", "limit": 1.0, "ok": false},
        });
        let text = report(&result, &problem(), true).unwrap();
        assert!(text.contains("VIOLATED"), "{text}");
        assert!(text.contains("J 1.0000 (start design)"));
    }
}
