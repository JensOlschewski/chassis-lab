//! `clab optimize`: grades or optimizes a design against a problem file.
//!
//! The optimization runs inside the compiled MATLAB solver (`mks_solver
//! --optimize`); this side validates the problem, starts the solver and reports.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::config::problem::Problem;
use clap::Args;
use serde_json::Value;

use crate::matlab::{self, file_path};

#[derive(Debug, Args)]
pub(crate) struct OptimizeArgs {
    /// Optimization problem TOML.
    problem: Option<PathBuf>,
    /// Start design JSON. Also the reference of `goal` objectives.
    #[arg(long, value_name = "FILE")]
    design: PathBuf,
    /// Write the optimized design to PATH: a file, or a directory that receives
    /// `<problem>-optimized.json`. Without PATH, the file goes to the current
    /// directory. Give the problem before this flag.
    #[arg(long, value_name = "PATH", num_args = 0..=1, conflicts_with = "dry_run")]
    json: Option<Option<PathBuf>>,
    /// Grade the start design and stop.
    #[arg(long)]
    dry_run: bool,
    /// Replace an existing --json file.
    #[arg(long, requires = "json")]
    force: bool,
    /// Show optimizer progress on stderr.
    #[arg(long)]
    progress: bool,
}

pub(crate) fn optimize(args: OptimizeArgs) -> Result<()> {
    let Some(problem_path) = &args.problem else {
        match &args.json {
            Some(Some(path)) => bail!("give <PROBLEM> before --json (got {})", path.display()),
            _ => bail!("missing <PROBLEM>"),
        }
    };
    let problem_file = file_path(problem_path, "problem")?;
    let design = file_path(&args.design, "design")?;
    let problem = Problem::load(&problem_file)?;
    let output = args
        .json
        .as_ref()
        .map(|path| matlab::output_path(path.as_deref(), problem_path, "optimized"))
        .transpose()?;
    if let Some(output) = &output {
        matlab::check_output(
            output,
            &[(&problem_file, "problem"), (&design, "design")],
            args.force,
        )?;
    }

    let mut problem_json = tempfile::Builder::new()
        .suffix(".json")
        .tempfile()
        .context("cannot create a scratch file")?;
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
                text(&o["kind"]).to_owned(),
                format!("{}", number(&o["weight"])),
                format!("{:.6}", number(&o["reference"])),
                format!("{:.6}", number(&o["value"])),
                format!("{:.4}", number(&o["d"])),
                format!("{:.3}", number(&o["grade"])),
            ]
        })
        .collect();
    writeln!(out, "\nobjectives")?;
    out += &table(
        &["name", "kind", "weight", "start", "value", "d", "grade"],
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
        writeln!(out, "grade {:.4} (start design)", number(&result["grade"]))?;
        let scales: std::collections::BTreeSet<&str> = items(&result["objectives"])
            .iter()
            .map(|o| text(&o["rule"]))
            .collect();
        writeln!(out, "scale rule: {}", Vec::from_iter(scales).join(", "))?;
    } else {
        writeln!(
            out,
            "grade {:.4} -> {:.4}",
            number(&result["start_grade"]),
            number(&result["grade"])
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
goal = "minimize"
"#,
            Path::new("/base"),
        )
        .unwrap()
    }

    #[test]
    fn reports_single_objective_object_and_no_constraints() {
        let result: Value = serde_json::json!({
            "design": {"A": 5.0}, "start_design": {"A": 1.0},
            "objectives": {"name": "o", "kind": "minimize", "weight": 1, "reference": 3.0,
                "value": 2.0, "d": -0.33, "grade": 16.6, "rule": "reference"},
            "constraints": [],
            "grade": 16.6, "start_grade": 10.0,
            "fmincon": {"exitflag": 1, "iterations": 4, "evaluations": 12, "failed_evaluations": 0, "last_error": ""},
        });
        let text = report(&result, &problem(), false).unwrap();
        assert!(
            text.contains("A         1.000000  5.000000   [0, 10]"),
            "{text}"
        );
        assert!(text.contains("o     minimize"), "{text}");
        assert!(!text.contains("constraints"));
        assert!(text.contains("grade 10.0000 -> 16.6000"));
        assert!(text.contains("12 evaluations (0 failed)"));
    }

    #[test]
    fn flags_violated_constraints() {
        let result: Value = serde_json::json!({
            "design": {"A": 1.0}, "start_design": {"A": 1.0},
            "objectives": [], "grade": 10.0,
            "constraints": {"name": "c", "value": 2.0, "relation": "<=", "limit": 1.0, "ok": false},
        });
        let text = report(&result, &problem(), true).unwrap();
        assert!(text.contains("VIOLATED"), "{text}");
        assert!(text.contains("grade 10.0000 (start design)"));
    }
}
