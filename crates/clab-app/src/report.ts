import { invoke } from "@tauri-apps/api/core";
import { setupTools } from "./chart";
import logo from "./assets/flb-logo.svg?url";
import { Plot, PlotsFile, drawReportPlot } from "./reportplot";
import { ComparedMetric, ComparisonSection, Section, Series, renderComparison, renderSection } from "./sections";

interface ReportMetric {
  test: string;
  subtest: string | null;
  metric: string;
  unit: string;
  value: number | null;
}

interface ReportTest {
  test: string;
  subtest?: string;
}

interface AnalysisFile {
  command?: "analyze";
  tests: ReportTest[];
  metrics: ReportMetric[];
  warnings: string[];
}

/** A compared metric row as written by `clab compare`, one per candidate. */
interface CompareMetric {
  test: string;
  subtest: string | null;
  metric: string;
  unit: string;
  baseline_value: number | null;
  candidate_value: number | null;
  difference: number | null;
  allowed_difference: number | null;
  status: string;
}

interface CompareInput {
  input: { path: string };
  label?: string;
}

export interface CompareFile {
  command: "compare";
  tests: ReportTest[];
  baseline: CompareInput;
  candidates: (CompareInput & {
    metrics: CompareMetric[];
    tests: { test: string; subtest: string | null; status: string }[];
  })[];
}

export type ReportFile = AnalysisFile | CompareFile;

const readFile = (directory: string, name: string) => invoke<string>("read_report_file", { directory, name });

/** Read the saved report, never recompute results from its source files. */
export async function readReport(directory: string): Promise<ReportFile> {
  return JSON.parse(await readFile(directory, "report.json")) as ReportFile;
}

export const roleOf = (index: number) => (index === 0 ? "Baseline" : `Candidate ${index}`);

const keyOf = ({ test, subtest }: { test: string; subtest?: string | null }) => (subtest ? `${test}::${subtest}` : test);

/** The order of the live results; `report.json` lists tests sorted by key instead. */
const TEST_ORDER = ["svc", "knc::vertical", "knc::roll", "knc::longitudinal"];

const inResultOrder = (tests: ReportTest[]) =>
  [...tests].sort((a, b) => TEST_ORDER.indexOf(keyOf(a)) - TEST_ORDER.indexOf(keyOf(b)));

const titleOf = (test: ReportTest) =>
  test.test === "svc" ? "SVC" : `KnC ${test.subtest!.charAt(0).toUpperCase()}${test.subtest!.slice(1)}`;

/** Cards from `report.json`: metrics and warnings grouped by test and subtest, as in the Analyze tab. */
export function sectionsFromReport(report: AnalysisFile): Section[] {
  const warnings = [...report.warnings];
  const sections: Section[] = inResultOrder(report.tests).map((test) => {
    const key = keyOf(test);
    const prefix = `${key}: `;
    const own = warnings.filter((warning) => warning.startsWith(prefix));
    own.forEach((warning) => warnings.splice(warnings.indexOf(warning), 1));
    return {
      test: test.test,
      subtest: test.subtest,
      title: titleOf(test),
      metrics: report.metrics
        .filter((metric) => keyOf(metric) === key)
        .map(({ metric, unit, value }) => ({ name: metric, unit, value })),
      warnings: own.map((warning) => warning.slice(prefix.length)),
    };
  });
  // Warnings that name no test would otherwise vanish.
  if (warnings.length > 0) sections.push({ title: "General", metrics: [], warnings });
  return sections;
}

/** Comparison cards from `report.json`: the rows of every candidate regrouped per metric, as `clab compare --json` prints them. */
export function comparisonFromReport(report: CompareFile): { series: Series[]; sections: ComparisonSection[] } {
  const series = [report.baseline, ...report.candidates].map(({ input, label }, index) => ({
    path: input.path,
    name: label ?? roleOf(index),
  }));
  const sections = inResultOrder(report.tests).map((test) => {
    const key = keyOf(test);
    const rows = report.candidates.map((candidate) => ({
      skipped: candidate.tests.some((entry) => keyOf(entry) === key && entry.status === "not compared"),
      metrics: candidate.metrics.filter((metric) => keyOf(metric) === key),
    }));
    const names = [...new Set(rows.flatMap(({ metrics }) => metrics.map(({ metric }) => metric)))];
    const metrics: ComparedMetric[] = names.map((name) => {
      const found = rows.map(({ metrics }) => metrics.find(({ metric }) => metric === name));
      const first = found.find((row) => row !== undefined)!;
      return {
        name,
        unit: first.unit,
        baseline: first.baseline_value,
        allowed_difference: found.find((row) => row?.allowed_difference != null)?.allowed_difference ?? null,
        candidates: found.map((row, index) =>
          rows[index].skipped
            ? { value: null, difference: null, status: "not compared" }
            : { value: row?.candidate_value ?? null, difference: row?.difference ?? null, status: row?.status ?? "missing" },
        ),
      };
    });
    return { title: titleOf(test), metrics, warnings: [], test: test.test, subtest: test.subtest };
  });
  return { series, sections };
}

/** The report body of the Inspect pane: cards and plots of a report folder. */
export function setupReport(show: (message: string, ok?: boolean) => void) {
  const sectionsRoot = document.querySelector<HTMLElement>("#report-sections")!;
  const viewer = document.querySelector<HTMLElement>("#report-viewer")!;
  const picker = document.querySelector<HTMLSelectElement>("#report-plot")!;
  const chart = document.querySelector<HTMLElement>("#report-chart")!;

  let plots: { plot: Plot; subtest: string }[] = [];
  let axes: string[] = [];

  const selectedTool = () => document.querySelector<HTMLElement>("#report-tools [aria-pressed=true]")?.dataset.tool;
  const draw = async () => {
    const entry = plots[Number(picker.value)];
    if (!entry) return;
    highlight(entry.plot.metrics ?? []);
    axes = await drawReportPlot(chart, entry.plot, logo, selectedTool());
  };

  /** Marks the table rows of `metrics`, opening their cards. */
  const highlight = (metrics: string[]) => {
    sectionsRoot.querySelectorAll("tr.highlight").forEach((row) => row.classList.remove("highlight"));
    for (const metric of metrics) {
      const row = sectionsRoot.querySelector<HTMLElement>(`tr[data-metric="${metric}"]`);
      row?.classList.add("highlight");
      row?.closest("details")?.setAttribute("open", "");
    }
  };

  setupTools(chart, document.querySelector<HTMLElement>("#report-tools")!, () => axes, show);
  picker.addEventListener("change", () => draw().catch((error) => show(String(error), false)));
  let resizing: number | undefined;
  window.addEventListener("resize", () => {
    window.clearTimeout(resizing);
    resizing = window.setTimeout(() => !viewer.hidden && draw(), 150);
  });

  /** Plots of every KnC subtest the report has; a subtest without a plots file is named in the status. */
  async function loadPlots(directory: string, tests: ReportTest[]): Promise<string[]> {
    plots = [];
    const groups: HTMLOptGroupElement[] = [];
    const missing: string[] = [];
    for (const test of inResultOrder(tests).filter((test) => test.test === "knc")) {
      const stems = test.subtest === "longitudinal" ? ["longitudinal-b0", "longitudinal-b1"] : [test.subtest!];
      for (const stem of stems) {
        const label = stems.length > 1 ? `${titleOf(test)} (brakes ${stem.endsWith("b1") ? "on" : "off"})` : titleOf(test);
        try {
          const file = JSON.parse(await readFile(directory, `knc-${stem}.plots.json`)) as PlotsFile;
          const group = document.createElement("optgroup");
          group.label = label;
          for (const plot of file.plots) {
            group.append(new Option(plot.name, String(plots.length)));
            plots.push({ plot, subtest: test.subtest! });
          }
          groups.push(group);
        } catch {
          missing.push(label);
        }
      }
    }
    picker.replaceChildren(...groups);
    return missing;
  }

  return {
    clear() {
      sectionsRoot.replaceChildren();
      viewer.hidden = true;
    },
    /** Shows the report in `directory`, starting at the plots of `subtest` if given. The pane must be visible. */
    async open(directory: string, subtest?: string) {
      this.clear();
      show("Opening…");
      try {
        const report = await readReport(directory);
        if (report.command === "compare") {
          const { series, sections } = comparisonFromReport(report);
          sectionsRoot.replaceChildren(...sections.map((section) => renderComparison(section, series)));
        } else {
          sectionsRoot.replaceChildren(...sectionsFromReport(report).map((section) => renderSection(section)));
        }
        const missing = await loadPlots(directory, report.tests);
        viewer.hidden = plots.length === 0;
        show(
          missing.length > 0
            ? `No plot data for ${missing.join(", ")}; run ${report.command === "compare" ? "Compare" : "Analyze"} with a report folder again.`
            : "",
        );
        const first = plots.findIndex((entry) => entry.subtest === subtest);
        if (first >= 0) picker.value = String(first);
        if (plots.length > 0) await draw();
      } catch (error) {
        show(`Failed to open report: ${error}`, false);
      }
    },
  };
}
