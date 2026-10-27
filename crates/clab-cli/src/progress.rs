//! Spinner status line for the slow plot-rendering and report-writing phases
//! that run after `analyze`/`compare` print their metrics table. Those phases
//! are otherwise silent, which makes a slow run with `--report` look stuck.
//!
//! Silent unless stderr is a terminal, so piped/CI output is unchanged.

use std::io::{IsTerminal, Write};
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub(crate) struct ReportProgress {
    bar: Option<ProgressBar>,
    multi: Option<MultiProgress>,
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("  {spinner:.cyan} {msg}").expect("valid spinner template")
}

impl ReportProgress {
    /// `active` is whether this run will reach the slow phases at all
    /// (i.e. a report output was requested).
    pub(crate) fn new(active: bool) -> Self {
        if !active || !std::io::stderr().is_terminal() {
            return Self {
                bar: None,
                multi: None,
            };
        }

        println!("\nReport generation");
        std::io::stdout().flush().ok();
        let multi = MultiProgress::new();
        let bar = multi.add(ProgressBar::new_spinner());
        bar.set_style(spinner_style());
        bar.enable_steady_tick(Duration::from_millis(80));
        Self {
            bar: Some(bar),
            multi: Some(multi),
        }
    }

    /// Start a new phase, replacing the spinner's message.
    pub(crate) fn phase(&self, message: impl Into<String>) {
        if let Some(bar) = &self.bar {
            bar.set_message(message.into());
        }
    }

    /// Mark the current phase done and leave its checkmark in scrollback.
    pub(crate) fn done(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.println(format!("  \x1b[32m✓\x1b[0m {message}"));
        }
    }

    /// Clear the spinner line once every phase is done.
    pub(crate) fn finish(&self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }

    /// Two independent plot phases occupy separate live lines. The regular
    /// spinner stays hidden until report writing begins.
    pub(crate) fn parallel_phases(&self, labels: &[&str]) -> Vec<ProgressBar> {
        let (Some(multi), Some(bar)) = (&self.multi, &self.bar) else {
            return Vec::new();
        };
        bar.set_style(ProgressStyle::with_template("").expect("valid empty style"));
        labels
            .iter()
            .map(|label| {
                let child = multi.insert_before(bar, ProgressBar::new_spinner());
                child.set_style(spinner_style());
                child.set_message(format!("Generating {label} plots…"));
                child.enable_steady_tick(Duration::from_millis(80));
                child
            })
            .collect()
    }

    pub(crate) fn parallel_done(&self, child: Option<&ProgressBar>, label: &str) {
        if let Some(child) = child {
            child.println(format!("  \x1b[32m✓\x1b[0m {label} plots"));
            child.finish_and_clear();
        }
    }

    pub(crate) fn end_parallel(&self) {
        if let Some(bar) = &self.bar {
            bar.set_style(spinner_style());
        }
    }
}
