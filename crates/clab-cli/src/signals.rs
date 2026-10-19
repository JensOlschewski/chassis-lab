use anyhow::{Result, bail};
use clab_core::{
    config::KNC_TESTS,
    conventions::{SignalConvention, roll_conventions, svc_conventions, vertical_conventions},
};
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Domain {
    Svc,
    Knc,
}

type Conventions = std::collections::BTreeMap<String, SignalConvention>;

pub(crate) fn mapping(domain: Option<Domain>, test: Option<&str>, template: bool) -> Result<()> {
    if test.is_some_and(|test| !KNC_TESTS.contains(&test)) {
        bail!("unsupported KnC test; expected {}", KNC_TESTS.join(" or "));
    }
    if test.is_some() && matches!(domain, Some(Domain::Svc)) {
        bail!("--test requires --domain knc");
    }
    let svc = svc_conventions();
    let knc: Vec<(&str, Conventions)> = KNC_TESTS
        .into_iter()
        .filter(|name| test.is_none_or(|test| test == *name))
        .map(|name| {
            let conventions = match name {
                "roll" => roll_conventions(),
                _ => vertical_conventions(),
            };
            (name, conventions)
        })
        .collect();
    if template {
        print_template(domain, test.is_some(), &svc, &knc);
    } else {
        print_list(domain, test.is_some(), &svc, &knc);
    }
    Ok(())
}

/// Without `--test`, wheel travel is shared by every KnC test and listed once
/// under `knc`; with it, all signals of that test are listed under the test.
fn print_list(
    domain: Option<Domain>,
    single_test: bool,
    svc: &Conventions,
    knc: &[(&str, Conventions)],
) {
    if domain.is_none() || matches!(domain, Some(Domain::Svc)) {
        println!("svc:");
        print_rows(2, svc.iter());
    }
    if domain.is_none() || matches!(domain, Some(Domain::Knc)) {
        println!("knc:");
        if !single_test {
            print_rows(2, knc[0].1.iter().filter(|(name, _)| !test_signal(name)));
        }
        for (test, conventions) in knc {
            println!("  {test}:");
            print_rows(
                4,
                conventions
                    .iter()
                    .filter(|(name, _)| single_test || test_signal(name)),
            );
        }
    }
}

fn print_rows<'a, I>(indent: usize, rows: I)
where
    I: Iterator<Item = (&'a String, &'a SignalConvention)>,
{
    let rows = rows.collect::<Vec<_>>();
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for (name, convention) in rows {
        println!(
            "{spaces}{name:<width$}  {}",
            convention.unit,
            spaces = " ".repeat(indent)
        );
    }
}

fn test_signal(name: &str) -> bool {
    !name.ends_with("_travel")
}

fn print_template(
    domain: Option<Domain>,
    single_test: bool,
    svc: &Conventions,
    knc: &[(&str, Conventions)],
) {
    if domain.is_none() || matches!(domain, Some(Domain::Svc)) {
        for (name, convention) in svc {
            signal_table("svc", name, convention);
        }
    }
    if domain.is_none() || matches!(domain, Some(Domain::Knc)) {
        if !single_test {
            for (name, convention) in &knc[0].1 {
                if !test_signal(name) {
                    signal_table("knc", name, convention);
                }
            }
        }
        for (test, conventions) in knc {
            for (name, convention) in conventions {
                if single_test || test_signal(name) {
                    signal_table(&format!("knc.{test}"), name, convention);
                }
            }
        }
    }
}

fn signal_table(scope: &str, name: &str, convention: &SignalConvention) {
    println!("[{scope}.signals.{name}]");
    println!("entity = \"<entity>\"");
    println!("component = \"<component>\"");
    println!("unit = \"{}\"\n", convention.unit);
}
