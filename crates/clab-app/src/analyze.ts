import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { readReport, sectionsFromReport } from "./report";
import { BadgeState, Section, createBadge, renderSection, setBadge } from "./sections";
import { describe, errorLine, runStreaming } from "./worker";

type PhaseState = "start" | "done" | "error";

/** One `--json` stdout line: a result `section`, a `progress` event, or something else (`report`). */
export function parseLine(line: string): { section?: Section; progress?: { phase: string; state: PhaseState } } {
  return line.startsWith("{") ? JSON.parse(line) : {};
}

const BUNDLE_PHASE = "Writing report bundle";

/** Pasted paths often carry whitespace or the quotes added by "Copy as path". */
const clean = (path: string) => path.trim().replace(/^"|"$/g, "").trim();

const join = (directory: string, ...parts: string[]) => {
  const separator = directory.includes("\\") && !directory.includes("/") ? "\\" : "/";
  return [directory.replace(/[\\/]+$/, ""), ...parts].join(separator);
};

const parentOf = (path: string) => {
  const end = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return end > 0 ? path.slice(0, end) : ".";
};

/** A report badge follows the phases its section depends on: its own plots, then the bundle write. */
interface ReportBadge {
  badge: HTMLElement;
  phases: string[];
}

/** The form as stored in a session file: paths only, never results. */
export interface AnalyzeState {
  config: string | null;
  input: string | null;
  generate: boolean;
  report: string | null;
}

/** `showReport` opens a report folder in Inspect, at the plots of `test` if given. */
export function setupAnalyze(showReport: (directory: string, test?: string) => void) {
  const form = document.querySelector<HTMLFormElement>("#analyze-form")!;
  const status = document.querySelector<HTMLElement>("#analyze-status")!;
  const sections = document.querySelector<HTMLElement>("#analyze-sections")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const reportButton = document.querySelector<HTMLButtonElement>("#analyze-show-report")!;
  let shown: string | null = null;
  let revision = 0;
  reportButton.addEventListener("click", () => shown && showReport(shown));

  const show = (message: string, ok: boolean) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const phaseStates = new Map<string, PhaseState>();
  const reportBadges: ReportBadge[] = [];
  const reportState = ({ phases }: ReportBadge): BadgeState => {
    const states = phases.map((phase) => phaseStates.get(phase));
    if (states.includes("error")) return "error";
    return states.every((state) => state === "done") ? "ok" : "running";
  };
  const refreshBadges = () => reportBadges.forEach((report) => setBadge(report.badge, reportState(report)));
  /** Phases still open when the run ends are settled by its exit code; SVC-only runs announce none. */
  const settlePhases = (ok: boolean) => {
    reportBadges.flatMap(({ phases }) => phases).forEach((phase) => {
      const state = phaseStates.get(phase);
      if (state !== "done" && state !== "error") phaseStates.set(phase, ok ? "done" : "error");
    });
    refreshBadges();
  };

  setupBrowse(form);

  const reportOptions = form.querySelector<HTMLFieldSetElement>("#analyze-report")!;
  const generate = form.elements.namedItem("generate") as HTMLInputElement;
  const folder = form.elements.namedItem("report") as HTMLInputElement;
  // Disabled controls stay out of FormData, so an unchecked box sends no report folder.
  const syncGenerate = () => (reportOptions.disabled = !generate.checked);
  generate.addEventListener("change", syncGenerate);

  /** The report folder to write, `null` for none; `undefined` when the user cancels a confirmation. */
  async function confirmReport(input: string): Promise<{ report: string | null; force: boolean } | undefined> {
    if (!generate.checked) return { report: null, force: false };
    const chosen = clean(folder.value);
    const report = chosen || join(parentOf(input), "report");
    if ((await invoke<string>("path_kind", { path: report })) !== "missing") {
      const replace = await ask(`The report folder already exists:\n${report}\n\nIts contents will be replaced.`, {
        title: "Replace report folder?",
        kind: "warning",
        okLabel: "Replace",
        cancelLabel: "Cancel",
      });
      if (!replace) return undefined;
      return { report, force: true };
    }
    if (!chosen) {
      const create = await ask(`No report folder was given. Create it next to the input?\n${report}`, {
        title: "Create report folder?",
        okLabel: "Create",
        cancelLabel: "Cancel",
      });
      if (!create) return undefined;
    }
    folder.value = report;
    return { report, force: false };
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    try {
      const data = new FormData(form);
      const config = clean(String(data.get("config") ?? ""));
      const input = clean(String(data.get("input") ?? ""));
      const confirmed = await confirmReport(input);
      if (!confirmed) return;
      const { report, force } = confirmed;
      revision++;

      sections.replaceChildren();
      sections.classList.remove("finished");
      reportButton.hidden = true;
      phaseStates.clear();
      reportBadges.length = 0;
      show("Analyzing…", true);
      const outcome = await runStreaming("run_analyze", { config, input, report, force }, (line) => {
        const { section, progress } = parseLine(line);
        if (section) {
          const knc = section.domain === "knc";
          let status;
          if (report) {
            const badge = createBadge("running");
            reportBadges.push({ badge, phases: knc ? [`${section.title} plots`, BUNDLE_PHASE] : [BUNDLE_PHASE] });
            status = {
              badge,
              plots: knc ? join(report, "plots", "knc", String(section.test)) : undefined,
              open: knc ? () => showReport(report, String(section.test)) : undefined,
            };
          }
          sections.append(renderSection(section, status));
        }
        if (progress) {
          phaseStates.set(progress.phase, progress.state);
          refreshBadges();
        }
      });
      const { ok, message } = describe(outcome);
      settlePhases(ok);
      if (ok && report) {
        shown = report;
        sections.classList.add("finished");
        reportButton.hidden = false;
      }
      show(ok ? message : (errorLine(outcome.stderr) ?? message), ok);
    } catch (error) {
      settlePhases(false);
      show(`Failed to run clab: ${error}`, false);
    } finally {
      submit.disabled = false;
    }
  });

  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;
  const saved = (name: string) => clean(field(name).value) || null;

  return {
    get(): AnalyzeState {
      return { config: saved("config"), input: saved("input"), generate: generate.checked, report: saved("report") };
    },
    /** Restore saved report results as historical, never as a new analysis. */
    async set(state: AnalyzeState, inspectPath?: string | null): Promise<void> {
      const current = ++revision;
      field("config").value = state.config ?? "";
      field("input").value = state.input ?? "";
      generate.checked = state.generate;
      folder.value = state.report ?? "";
      syncGenerate();
      sections.replaceChildren();
      sections.classList.remove("finished");
      reportButton.hidden = true;
      shown = null;
      phaseStates.clear();
      reportBadges.length = 0;
      show("", true);

      // Older sessions can have the report in Inspect but no Analyze report
      // destination. Only associate that report if both source paths match.
      const directory = state.report ?? inspectPath;
      if (!directory) return;
      try {
        if ((await invoke<string>("path_kind", { path: directory })) !== "report") {
          if (current === revision && state.report) show("Saved report is unavailable. Run Analyze to generate it.", false);
          return;
        }
        const report = await readReport(directory);
        if (current !== revision) return;
        if (report.command !== "analyze") return;
        if (!state.report && (report.inputs?.config?.path !== state.config || report.inputs?.run?.path !== state.input)) return;
        const cards = sectionsFromReport(report).map((section) => renderSection(section, {
          badge: createBadge("ok"),
          plots: section.domain === "knc" ? join(directory, "plots", "knc", String(section.test)) : undefined,
          open: section.domain === "knc" ? () => showReport(directory, String(section.test)) : undefined,
        }));
        sections.replaceChildren(...cards);
        sections.classList.add("finished");
        folder.value = directory;
        shown = directory;
        reportButton.hidden = false;
        show("Saved report results (full catalog). Not rerun or checked against current inputs.", true);
      } catch (error) {
        if (current === revision && state.report) show(`Could not restore saved report: ${error}`, false);
      }
    },
  };
}
