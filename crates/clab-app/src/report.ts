import { invoke } from "@tauri-apps/api/core";
import { setupTools } from "./chart";
import logo from "./assets/flb-logo.svg?url";
import { Plot, PlotsFile, drawReportPlot } from "./reportplot";
import { Section, renderSection } from "./sections";

interface ReportMetric {
  domain: string;
  test: string | null;
  metric: string;
  unit: string;
  value: number | null;
}

interface ReportDomain {
  domain: string;
  test?: string;
}

interface ReportFile {
  domains: ReportDomain[];
  metrics: ReportMetric[];
  warnings: string[];
}

const readFile = (directory: string, name: string) => invoke<string>("read_report_file", { directory, name });

const keyOf = ({ domain, test }: { domain: string; test?: string | null }) => (test ? `${domain}::${test}` : domain);

const titleOf = (domain: ReportDomain) =>
  domain.domain === "svc" ? "SVC" : `KnC ${domain.test!.charAt(0).toUpperCase()}${domain.test!.slice(1)}`;

/** Cards from `report.json`: metrics and warnings grouped by domain and test, as in the Analyze tab. */
export function sectionsFromReport(report: ReportFile): Section[] {
  const warnings = [...report.warnings];
  const sections = report.domains.map((domain) => {
    const key = keyOf(domain);
    const prefix = `${key}: `;
    const own = warnings.filter((warning) => warning.startsWith(prefix));
    own.forEach((warning) => warnings.splice(warnings.indexOf(warning), 1));
    return {
      title: titleOf(domain),
      metrics: report.metrics
        .filter((metric) => keyOf(metric) === key)
        .map(({ metric, unit, value }) => ({ name: metric, unit, value })),
      warnings: own.map((warning) => warning.slice(prefix.length)),
    };
  });
  // Warnings that name no domain would otherwise vanish.
  if (warnings.length > 0) sections.push({ title: "General", metrics: [], warnings });
  return sections;
}

/** The report body of the Inspect pane: cards and plots of a report folder. */
export function setupReport(show: (message: string, ok?: boolean) => void) {
  const sectionsRoot = document.querySelector<HTMLElement>("#report-sections")!;
  const viewer = document.querySelector<HTMLElement>("#report-viewer")!;
  const picker = document.querySelector<HTMLSelectElement>("#report-plot")!;
  const chart = document.querySelector<HTMLElement>("#report-chart")!;

  let plots: { plot: Plot; test: string }[] = [];
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

  /** Plots of every KnC test the report has; a test without a plots file is named in the status. */
  async function loadPlots(directory: string, domains: ReportDomain[]): Promise<string[]> {
    plots = [];
    const groups: HTMLOptGroupElement[] = [];
    const missing: string[] = [];
    for (const domain of domains.filter((domain) => domain.domain === "knc")) {
      try {
        const file = JSON.parse(await readFile(directory, `knc-${domain.test}.plots.json`)) as PlotsFile;
        const group = document.createElement("optgroup");
        group.label = titleOf(domain);
        for (const plot of file.plots) {
          group.append(new Option(plot.title, String(plots.length)));
          plots.push({ plot, test: domain.test! });
        }
        groups.push(group);
      } catch {
        missing.push(titleOf(domain));
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
    /** Shows the report in `directory`, starting at the plots of `test` if given. The pane must be visible. */
    async open(directory: string, test?: string) {
      this.clear();
      show("Opening…");
      try {
        const report = JSON.parse(await readFile(directory, "report.json")) as ReportFile;
        sectionsRoot.replaceChildren(...sectionsFromReport(report).map((section) => renderSection(section)));
        const missing = await loadPlots(directory, report.domains);
        viewer.hidden = plots.length === 0;
        show(missing.length > 0 ? `No plot data for ${missing.join(", ")}; run Analyze with a report folder again.` : "");
        const first = plots.findIndex((entry) => entry.test === test);
        if (first >= 0) picker.value = String(first);
        if (plots.length > 0) await draw();
      } catch (error) {
        show(`Failed to open report: ${error}`, false);
      }
    },
  };
}
