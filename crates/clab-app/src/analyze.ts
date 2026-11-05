import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { pick, setupBrowse } from "./browse";
import { swatch } from "./colors";
import { readReport, roleOf, sectionsFromReport } from "./report";
import {
  BadgeState,
  ComparisonSection,
  ReportStatus,
  Section,
  createBadge,
  renderComparison,
  renderSection,
  setBadge,
} from "./sections";
import { describe, errorLine, runStreaming } from "./worker";

type PhaseState = "start" | "done" | "error";

/** One `--json` stdout line: a result `section`, a `progress` event, the closing `comparison`, or something else (`report`). */
export function parseLine(line: string): {
  section?: Section & ComparisonSection;
  progress?: { phase: string; state: PhaseState };
  comparison?: { status: string };
} {
  return line.startsWith("{") ? JSON.parse(line) : {};
}

const BUNDLE_PHASE = "Writing report bundle";
const INPUT_EXTENSIONS = ["toml", "h5", "hdf5", "json"];

/** `clab compare` exits with 1 only when tolerance checks failed; its report is still written. */
const TOLERANCE_FAILURE = 1;

/** Final status of a comparison that exited normally. */
const COMPARISON_MESSAGES: Record<string, string> = {
  passed: "Compared: all metrics within tolerance",
  incomplete: "Compared: some metrics or domains are missing",
  informational: "Compared: differences only, no tolerances applied",
};

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
  /** One input is analyzed; more are compared, the first being the baseline. */
  inputs: string[];
  /** Legend name per input, in order; empty keeps the default. */
  names: string[];
  tolerances: boolean;
  generate: boolean;
  report: string | null;
}

interface ChangedInput {
  path: string;
  change: "missing" | "changed";
}

/** Saved results are from an earlier run; say whether the inputs they came from still match. */
async function describeSaved(directory: string, subject: string, advice: string): Promise<{ message: string; ok: boolean }> {
  const changed = await invoke<ChangedInput[]>("changed_report_inputs", { directory });
  if (changed.length === 0) return { message: `${subject}. Inputs unchanged since the run. ${advice}`, ok: true };
  const list = changed.map(({ path, change }) => `${path} (${change})`).join(", ");
  return { message: `${subject}. Inputs changed since the run: ${list}. Run again to update.`, ok: false };
}

/** `showReport` opens a report folder in Inspect, at the plots of `test` if given. */
export function setupAnalyze(showReport: (directory: string, test?: string) => void) {
  const form = document.querySelector<HTMLFormElement>("#analyze-form")!;
  const status = document.querySelector<HTMLElement>("#analyze-status")!;
  const sections = document.querySelector<HTMLElement>("#analyze-sections")!;
  const inputsRoot = document.querySelector<HTMLElement>("#analyze-inputs")!;
  const addInput = document.querySelector<HTMLButtonElement>("#add-input")!;
  const tolerancesOption = document.querySelector<HTMLElement>("#analyze-tolerances")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const reportButton = document.querySelector<HTMLButtonElement>("#analyze-show-report")!;
  const plotsButton = document.querySelector<HTMLButtonElement>("#analyze-open-plots")!;
  let shown: string | null = null;
  let revision = 0;
  reportButton.addEventListener("click", () => shown && showReport(shown));

  const show = (message: string, ok: boolean) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  plotsButton.addEventListener("click", async () => {
    if (!shown) return;
    const current = revision;
    try {
      await invoke("open_report_plots", { directory: shown });
    } catch (error) {
      if (current === revision) show(`Could not open plots folder: ${error}`, false);
    }
  });

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
  const tolerances = form.elements.namedItem("tolerances") as HTMLInputElement;
  const folder = form.elements.namedItem("report") as HTMLInputElement;
  // Disabled controls stay out of FormData, so an unchecked box sends no report folder.
  const syncGenerate = () => (reportOptions.disabled = !generate.checked);
  generate.addEventListener("change", syncGenerate);

  const rows = () => [...inputsRoot.querySelectorAll<HTMLElement>(".input-row")];
  const pathField = (row: HTMLElement) => row.querySelector<HTMLInputElement>(".path")!;
  const nameField = (row: HTMLElement) => row.querySelector<HTMLInputElement>(".name")!;
  const comparing = () => rows().length > 1;
  /** The Baseline/Candidate N role of the `index`th row, which names its column unless the user did. */

  /** Tells the session that the input rows changed; each field reports its own edits. */
  const changed = () => inputsRoot.dispatchEvent(new Event("change", { bubbles: true }));

  /** Marks rows with their plot colors, and switches the form between Analyze and Compare. */
  const syncRows = () => {
    rows().forEach((row, index) => {
      const marker = row.querySelector<HTMLElement>(".role")!;
      marker.hidden = !comparing();
      marker.replaceChildren(swatch(index));
      pathField(row).ariaLabel = comparing() ? roleOf(index) : "Input";
      row.querySelector<HTMLButtonElement>("[data-remove]")!.hidden = !comparing();
    });
    tolerancesOption.hidden = !comparing();
    submit.textContent = comparing() ? "Compare" : "Analyze";
  };

  function addRow(path = "", name = ""): HTMLInputElement {
    const row = document.createElement("div");
    row.className = "input-row";
    const role = document.createElement("span");
    role.className = "role";
    const label = document.createElement("input");
    label.className = "name";
    label.placeholder = "name (optional)";
    label.ariaLabel = "Name";
    label.maxLength = 64;
    label.value = name;
    const input = document.createElement("input");
    input.className = "path";
    input.required = true;
    input.placeholder = "run .toml, .h5, or SVC .json";
    input.value = path;
    const browse = document.createElement("button");
    browse.type = "button";
    browse.textContent = "Browse…";
    browse.addEventListener("click", async () => {
      const selected = await pick(false, INPUT_EXTENSIONS);
      if (!selected) return;
      input.value = selected;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "×";
    remove.ariaLabel = "Remove input";
    remove.dataset.remove = "";
    remove.addEventListener("click", () => {
      row.remove();
      syncRows();
      changed();
    });
    row.append(role, label, input, browse, remove);
    inputsRoot.append(row);
    syncRows();
    return input;
  }

  addInput.addEventListener("click", () => {
    addRow().focus();
    changed();
  });

  /** The report folder to write, `null` for none; `undefined` when the user cancels a confirmation. */
  async function confirmReport(input: string, compare: boolean): Promise<{ report: string | null; force: boolean } | undefined> {
    if (!generate.checked) return { report: null, force: false };
    const chosen = clean(folder.value);
    // A comparison next to the baseline must not replace its analysis report.
    const report = chosen || join(parentOf(input), compare ? "comparison" : "report");
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
      const create = await ask(`No report folder was given. Create it next to the ${compare ? "baseline" : "input"}?\n${report}`, {
        title: "Create report folder?",
        okLabel: "Create",
        cancelLabel: "Cancel",
      });
      if (!create) return undefined;
    }
    folder.value = report;
    return { report, force: false };
  }

  /** Where a KnC section's plots land; analysis plots also open in Inspect. */
  const kncStatus = (report: string, section: { test?: string | null }, compare: boolean, badge: HTMLElement): ReportStatus => {
    const test = String(section.test);
    return {
      badge,
      plots: compare ? join(report, "plots", "knc", test, "comparison") : join(report, "plots", "knc", test),
      open: () => showReport(report, test),
    };
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    let run = revision;
    try {
      const config = clean(String(new FormData(form).get("config") ?? ""));
      const inputs = rows().map((row) => clean(pathField(row).value));
      const names = rows().map((row) => nameField(row).value.trim());
      const series = inputs.map((path, index) => ({ path, name: names[index] || roleOf(index) }));
      const compare = inputs.length > 1;
      const confirmed = await confirmReport(inputs[0], compare);
      if (!confirmed || run !== revision) return;
      const { report, force } = confirmed;
      run = ++revision;

      sections.replaceChildren();
      sections.classList.remove("finished");
      reportButton.hidden = true;
      plotsButton.hidden = true;
      shown = null;
      phaseStates.clear();
      reportBadges.length = 0;
      show(compare ? "Comparing…" : "Analyzing…", true);
      let comparison: string | undefined;
      const request = { config, inputs, names, tolerances: compare && tolerances.checked, report, force };
      const outcome = await runStreaming("run_analyze", request, (line) => {
        if (run !== revision) return;
        const parsed = parseLine(line);
        const { section, progress } = parsed;
        if (section) {
          const knc = section.domain === "knc";
          let status: ReportStatus | undefined;
          if (report) {
            const badge = createBadge("running");
            reportBadges.push({ badge, phases: knc ? [`${section.title} plots`, BUNDLE_PHASE] : [BUNDLE_PHASE] });
            status = knc ? kncStatus(report, section, compare, badge) : { badge };
          }
          sections.append(compare ? renderComparison(section, series, status) : renderSection(section, status));
        }
        if (progress) {
          phaseStates.set(progress.phase, progress.state);
          refreshBadges();
        }
        if (parsed.comparison) comparison = parsed.comparison.status;
      });
      if (run !== revision) return;
      const toleranceFailed = compare && outcome.exit_code === TOLERANCE_FAILURE;
      const { ok, message } = describe(outcome);
      const completed = ok || toleranceFailed;
      settlePhases(completed);
      if (completed && report) {
        sections.classList.add("finished");
        shown = report;
        reportButton.hidden = false;
        plotsButton.hidden = false;
      }
      if (toleranceFailed) show("Compared: tolerance checks failed", false);
      else if (ok && compare) show(COMPARISON_MESSAGES[comparison ?? ""] ?? "Compared", comparison !== "incomplete");
      else show(ok ? message : (errorLine(outcome.stderr) ?? message), ok);
    } catch (error) {
      if (run !== revision) return;
      settlePhases(false);
      show(`Failed to run clab: ${error}`, false);
    } finally {
      submit.disabled = false;
    }
  });

  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;
  const saved = (name: string) => clean(field(name).value) || null;

  addRow();

  return {
    get(): AnalyzeState {
      const filled = rows().filter((row) => clean(pathField(row).value));
      return {
        config: saved("config"),
        inputs: filled.map((row) => clean(pathField(row).value)),
        names: filled.map((row) => nameField(row).value.trim()),
        tolerances: tolerances.checked,
        generate: generate.checked,
        report: saved("report"),
      };
    },
    /** Restore saved report results as historical, never as a new analysis. */
    async set(state: AnalyzeState): Promise<void> {
      const current = ++revision;
      field("config").value = state.config ?? "";
      inputsRoot.replaceChildren();
      (state.inputs.length > 0 ? state.inputs : [""]).forEach((input, index) => addRow(input, state.names[index]));
      tolerances.checked = state.tolerances;
      generate.checked = state.generate;
      folder.value = state.report ?? "";
      syncGenerate();
      sections.replaceChildren();
      sections.classList.remove("finished");
      reportButton.hidden = true;
      plotsButton.hidden = true;
      shown = null;
      phaseStates.clear();
      reportBadges.length = 0;
      show("", true);

      const directory = state.report;
      if (!directory) return;
      try {
        if ((await invoke<string>("path_kind", { path: directory })) !== "report") {
          if (current === revision) show("Saved report is unavailable. Run it again to generate it.", false);
          return;
        }
        const report = await readReport(directory);
        if (current !== revision) return;
        if (report.command === "compare") {
          shown = directory;
          reportButton.hidden = false;
          plotsButton.hidden = false;
          const { message, ok } = await describeSaved(directory, `Saved comparison report: ${directory}`, "Show it in Inspect.");
          if (current === revision) show(message, ok);
          return;
        }
        if (report.command !== "analyze") return;
        const cards = sectionsFromReport(report).map((section) => renderSection(section, {
          badge: createBadge("ok"),
          plots: section.domain === "knc" ? join(directory, "plots", "knc", String(section.test)) : undefined,
          open: section.domain === "knc" ? () => showReport(directory, String(section.test)) : undefined,
        }));
        sections.replaceChildren(...cards);
        sections.classList.add("finished");
        shown = directory;
        reportButton.hidden = false;
        plotsButton.hidden = false;
        const { message, ok } = await describeSaved(directory, "Saved report results", "Not rerun.");
        if (current === revision) show(message, ok);
      } catch (error) {
        if (current === revision) show(`Could not restore saved report: ${error}`, false);
      }
    },
  };
}
