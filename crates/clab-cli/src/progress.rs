//! Spinner status line for the slow plot-rendering and report-writing phases
//! that run after `analyze`/`compare` print their metrics table. Those phases
//! are otherwise silent, which makes a slow run with `--report` look stuck.
//!
//! Silent unless stderr is a terminal, so piped/CI output is unchanged.

use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::Result;
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

    pub(crate) fn println(&self, message: &str) {
        if let Some(multi) = &self.multi {
            multi.suspend(|| eprintln!("{message}"));
        } else {
            eprintln!("{message}");
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

    /// Render plot groups concurrently, updating each spinner as it completes.
    /// Results retain input order; all threads finish before errors propagate.
    pub(crate) fn plot_phases<J: Send, T: Send>(
        &self,
        jobs: Vec<(&str, J)>,
        render: impl Fn(J) -> Result<T> + Sync,
    ) -> Result<Vec<T>> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let labels = jobs.iter().map(|(label, _)| *label).collect::<Vec<_>>();
        let bars = self.parallel_phases(&labels);
        let result = std::thread::scope(|scope| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let render = &render;
            let handles = jobs
                .into_iter()
                .enumerate()
                .map(|(index, (_, job))| {
                    let sender = sender.clone();
                    scope.spawn(move || {
                        let _ = sender.send((index, render(job)));
                    })
                })
                .collect::<Vec<_>>();
            drop(sender);

            let mut results = Vec::with_capacity(handles.len());
            for (index, result) in receiver {
                if result.is_ok() {
                    self.parallel_done(bars.get(index), labels[index]);
                }
                results.push((index, result));
            }
            // Join every thread, even if one panics, to avoid a scope panic.
            let mut panicked = false;
            for handle in handles {
                panicked |= handle.join().is_err();
            }
            anyhow::ensure!(!panicked, "KnC plot worker panicked");
            results.sort_by_key(|(index, _)| *index);
            results.into_iter().map(|(_, result)| result).collect()
        });
        for bar in bars {
            bar.finish_and_clear();
        }
        if let Some(bar) = &self.bar {
            bar.set_style(spinner_style());
        }
        result
    }

    /// The regular spinner stays hidden until report writing begins.
    fn parallel_phases(&self, labels: &[&str]) -> Vec<ProgressBar> {
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

    fn parallel_done(&self, child: Option<&ProgressBar>, label: &str) {
        if let Some(child) = child {
            child.println(format!("  \x1b[32m✓\x1b[0m {label} plots"));
            child.finish_and_clear();
        }
    }
}

impl Drop for ReportProgress {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Barrier,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn plot_phases_run_concurrently_and_preserve_input_order() {
        let barrier = Barrier::new(2);
        let results = ReportProgress::new(false)
            .plot_phases(vec![("Vertical", 2), ("Roll", 1)], |value| {
                barrier.wait();
                Ok(value)
            })
            .unwrap();
        assert_eq!(results, [2, 1]);
    }

    #[test]
    fn plot_phases_join_other_jobs_before_returning_an_error() {
        let finished = AtomicBool::new(false);
        let result = ReportProgress::new(false).plot_phases(
            vec![("Vertical", false), ("Roll", true)],
            |fail| {
                if fail {
                    anyhow::bail!("render failed");
                }
                finished.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().to_string(), "render failed");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn plot_phases_return_thread_panics_as_errors() {
        let result =
            ReportProgress::new(false).plot_phases(vec![("Vertical", ())], |_| -> Result<()> {
                panic!("render panicked");
            });
        assert_eq!(result.unwrap_err().to_string(), "KnC plot worker panicked");
    }
}
