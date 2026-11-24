use anyhow::{Result, bail};
use clab_core::{
    config::KNC_TESTS,
    conventions::{
        SignalConvention, longitudinal_conventions, roll_conventions, svc_conventions,
        vertical_conventions,
    },
};
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Test {
    Svc,
    Knc,
}

type Conventions = std::collections::BTreeMap<String, SignalConvention>;

pub(crate) fn mapping(test: Option<Test>, subtest: Option<&str>, template: bool) -> Result<()> {
    if subtest.is_some_and(|subtest| !KNC_TESTS.contains(&subtest)) {
        bail!(
            "unsupported KnC subtest; expected {}",
            KNC_TESTS.join(" or ")
        );
    }
    if subtest.is_some() && matches!(test, Some(Test::Svc)) {
        bail!("--subtest requires --test knc");
    }
    let svc = svc_conventions();
    let knc: Vec<(&str, Conventions)> = KNC_TESTS
        .into_iter()
        .filter(|name| subtest.is_none_or(|subtest| subtest == *name))
        .map(|name| {
            let conventions = match name {
                "roll" => roll_conventions(),
                "longitudinal" => longitudinal_conventions(),
                _ => vertical_conventions(),
            };
            (name, conventions)
        })
        .collect();
    if template {
        print_template(test, subtest.is_some(), &svc, &knc);
    } else {
        print_list(test, subtest.is_some(), &svc, &knc);
    }
    Ok(())
}

/// Without `--subtest`, wheel travel is shared by every KnC subtest and listed once
/// under `knc`; with it, all signals of that subtest are listed under the subtest.
fn print_list(
    test: Option<Test>,
    single_subtest: bool,
    svc: &Conventions,
    knc: &[(&str, Conventions)],
) {
    if test.is_none() || matches!(test, Some(Test::Svc)) {
        println!("svc:");
        print_rows(2, svc.iter());
    }
    if test.is_none() || matches!(test, Some(Test::Knc)) {
        println!("knc:");
        if !single_subtest {
            print_rows(2, knc[0].1.iter().filter(|(name, _)| !test_signal(name)));
        }
        for (subtest, conventions) in knc {
            println!("  {subtest}:");
            print_rows(
                4,
                conventions
                    .iter()
                    .filter(|(name, _)| single_subtest || test_signal(name)),
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
    test: Option<Test>,
    single_subtest: bool,
    svc: &Conventions,
    knc: &[(&str, Conventions)],
) {
    if test.is_none() || matches!(test, Some(Test::Svc)) {
        for (name, convention) in svc {
            signal_table("svc", name, convention);
        }
    }
    if test.is_none() || matches!(test, Some(Test::Knc)) {
        if !single_subtest {
            for (name, convention) in &knc[0].1 {
                if !test_signal(name) {
                    signal_table("knc", name, convention);
                }
            }
        }
        for (subtest, conventions) in knc {
            for (name, convention) in conventions {
                if single_subtest || test_signal(name) {
                    signal_table(&format!("knc.{subtest}"), name, convention);
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
