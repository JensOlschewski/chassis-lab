use anyhow::{Result, bail};
use clab_core::conventions::{SignalConvention, svc_conventions, vertical_conventions};
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Domain {
    Svc,
    Knc,
}

pub(crate) fn mapping(domain: Option<Domain>, test: Option<&str>, template: bool) -> Result<()> {
    if test.is_some_and(|test| test != "vertical") {
        bail!("unsupported KnC test; expected vertical");
    }
    if test.is_some() && matches!(domain, Some(Domain::Svc)) {
        bail!("--test requires --domain knc");
    }
    let svc = svc_conventions();
    let knc = vertical_conventions();
    if template {
        print_template(domain, test.is_some(), &svc, &knc);
    } else {
        print_list(domain, test.is_some(), &svc, &knc);
    }
    Ok(())
}

fn print_list(
    domain: Option<Domain>,
    vertical: bool,
    svc: &std::collections::BTreeMap<String, SignalConvention>,
    knc: &std::collections::BTreeMap<String, SignalConvention>,
) {
    if domain.is_none() || matches!(domain, Some(Domain::Svc)) {
        println!("svc:");
        print_rows(2, svc.iter());
    }
    if domain.is_none() || matches!(domain, Some(Domain::Knc)) {
        println!("knc:");
        if !vertical {
            print_rows(2, knc.iter().filter(|(name, _)| name.ends_with("_travel")));
        }
        println!("  vertical:");
        print_rows(4, knc.iter().filter(|(name, _)| !name.ends_with("_travel")));
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

fn vertical_signal(name: &str) -> bool {
    !name.ends_with("_travel")
}

fn print_template(
    domain: Option<Domain>,
    vertical: bool,
    svc: &std::collections::BTreeMap<String, SignalConvention>,
    knc: &std::collections::BTreeMap<String, SignalConvention>,
) {
    if domain.is_none() || matches!(domain, Some(Domain::Svc)) {
        for (name, convention) in svc {
            signal_table("svc", name, convention);
        }
    }
    if domain.is_none() || matches!(domain, Some(Domain::Knc)) {
        for (name, convention) in knc {
            if !vertical && !vertical_signal(name) {
                signal_table("knc", name, convention);
            }
        }
        for (name, convention) in knc {
            if vertical || vertical_signal(name) {
                signal_table("knc.vertical", name, convention);
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
