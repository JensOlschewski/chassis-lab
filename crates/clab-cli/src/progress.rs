//! Spinner status line for the slow plot-rendering and report-writing phases
//! that run after `analyze`/`compare` print their metrics table. Those phases
//! are otherwise silent, which makes a slow run with `--report` look stuck.
//!
//! Silent unless stderr is a terminal, so piped/CI output is unchanged.

use std::io::{IsTerminal, Write};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

pub(crate) struct ReportProgress(Option<ProgressBar>);

impl ReportProgress {
    /// `active` is whether this run will reach the slow phases at all
    /// (i.e. a report output was requested).
    pub(crate) fn new(active: bool) -> Self {
        if !active || !std::io::stderr().is_terminal() {
            return Self(None);
        }

        println!("\nReport generation");
        std::io::stdout().flush().ok();
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                .expect("valid spinner template"),
        );
        bar.enable_steady_tick(Duration::from_millis(80));
        Self(Some(bar))
    }

    /// Start a new phase, replacing the spinner's message.
    pub(crate) fn phase(&self, message: impl Into<String>) {
        if let Some(bar) = &self.0 {
            bar.set_message(message.into());
        }
    }

    /// Mark the current phase done and leave its checkmark in scrollback.
    pub(crate) fn done(&self, message: &str) {
        if let Some(bar) = &self.0 {
            bar.println(format!("  \x1b[32m✓\x1b[0m {message}"));
        }
    }

    /// Clear the spinner line once every phase is done.
    pub(crate) fn finish(&self) {
        if let Some(bar) = &self.0 {
            bar.finish_and_clear();
        }
    }
}
