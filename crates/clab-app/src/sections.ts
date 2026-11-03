import { swatch } from "./colors";
import { formatValue } from "./format";

export interface Metric {
  name: string;
  unit: string;
  value: number | null;
}

export interface Section {
  title: string;
  metrics: Metric[];
  warnings: string[];
  domain?: string;
  test?: string | null;
}

export type BadgeState = "ok" | "warning" | "error" | "running";

export const element = <K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className = "") => {
  const node = document.createElement(tag);
  node.textContent = text;
  node.className = className;
  return node;
};

const BADGE_GLYPH = { ok: "✓", warning: "!", error: "✕", running: "" };

export function setBadge(badge: HTMLElement, state: BadgeState) {
  badge.className = `badge ${state}`;
  badge.textContent = BADGE_GLYPH[state];
}

export function createBadge(state: BadgeState) {
  const badge = element("span");
  setBadge(badge, state);
  return badge;
}

const labeled = (label: string, badge: HTMLElement) => {
  const node = element("span", "", "state");
  node.title = label;
  node.append(element("span", label), badge);
  return node;
};

/** The report badge is the caller's to update as the report is written; `plots` is where this section's plots land. */
export interface ReportStatus {
  badge: HTMLElement;
  plots?: string;
  /** Opens this section's plots; its button shows once the run succeeded. */
  open?: () => void;
}

/** One candidate's value of a compared metric, as printed by `clab compare --json`. */
export interface CandidateValue {
  value: number | null;
  difference: number | null;
  /** `passed`, `failed`, `missing`, `informational` (no tolerance applied), or `not compared`. */
  status: string;
}

export interface ComparedMetric {
  name: string;
  unit: string;
  baseline: number | null;
  allowed_difference: number | null;
  candidates: CandidateValue[];
}

export interface ComparisonSection {
  title: string;
  metrics: ComparedMetric[];
  warnings: string[];
  domain?: string;
  test?: string | null;
}

/** Names arrive qualified (`svc::front_left_mass_kg`); the card title already says where. */
const shortName = (name: string) => name.split("::").at(-1)!;

const valueCell = (row: HTMLTableRowElement, value: number | null, unit: string) => {
  const cell = row.insertCell();
  cell.textContent = formatValue(value, unit);
  cell.className = value === null ? "missing" : "num";
  if (value !== null) cell.title = String(value);
  return cell;
};

/** Card shell shared by analysis and comparison results; `body` goes between title and warnings. */
function renderCard(
  section: { title: string; warnings: string[] },
  result: { label: string; state: BadgeState },
  body: HTMLElement,
  report?: ReportStatus,
): HTMLDetailsElement {
  const card = element("details", "", "section");
  const summary = element("summary", "");
  summary.append(element("span", section.title));
  if (report) summary.append(labeled("Report", report.badge));
  summary.append(labeled(result.label, createBadge(result.state)));
  card.append(summary, body);

  if (section.warnings.length > 0) {
    const list = element("ul", "", "warnings");
    section.warnings.forEach((warning) => list.append(element("li", warning)));
    card.append(list);
  }
  if (report?.plots) {
    const location = element("p", `Plots: ${report.plots}`, "location");
    if (report.open) {
      const button = element("button", "Show plots", "show-plots");
      button.type = "button";
      button.addEventListener("click", report.open);
      location.append(button);
    }
    card.append(location);
  }
  return card;
}

/** A foldable card, always closed at first; its badges show whether it needs a look. */
export function renderSection(section: Section, report?: ReportStatus): HTMLDetailsElement {
  const missing = section.metrics.some((metric) => metric.value === null);
  const state = missing ? "error" : section.warnings.length > 0 ? "warning" : "ok";

  const table = element("table", "", "metrics");
  for (const metric of section.metrics) {
    const row = table.insertRow();
    row.dataset.metric = shortName(metric.name);
    row.insertCell().textContent = shortName(metric.name);
    valueCell(row, metric.value, metric.unit);
    row.insertCell().textContent = metric.value === null ? "" : metric.unit;
  }
  return renderCard(section, { label: "Analysis", state }, table, report);
}

const signed = (value: number, unit: string) => `${value > 0 ? "+" : ""}${formatValue(value, unit)}`;

/** A header cell with the series color used in the plots; 0 is the baseline. */
const seriesHeader = (text: string, index: number) => {
  const th = element("th", "");
  th.append(swatch(index), ` ${text}`);
  return th;
};

/**
 * A comparison card: the baseline value, the allowed difference when tolerances apply,
 * then each candidate's value and its difference from the baseline, colored by status.
 * `candidates` labels the columns, in the order the inputs were given.
 */
export function renderComparison(
  section: ComparisonSection,
  candidates: string[],
  report?: ReportStatus,
): HTMLDetailsElement {
  const statuses = section.metrics.flatMap((metric) => metric.candidates.map((candidate) => candidate.status));
  const state: BadgeState = statuses.some((status) => status === "failed" || status === "missing")
    ? "error"
    : statuses.includes("not compared") || section.warnings.length > 0
      ? "warning"
      : "ok";

  const withTolerances = section.metrics.some((metric) => metric.allowed_difference !== null);

  const table = element("table", "", "metrics comparison");
  const head = table.createTHead().insertRow();
  head.append(element("th"), seriesHeader("Baseline", 0));
  if (withTolerances) head.append(element("th", "Tolerance"));
  candidates.forEach((label, index) => {
    const th = seriesHeader(`Candidate ${index + 1}`, index + 1);
    th.colSpan = 2;
    th.title = label;
    head.append(th);
  });
  head.append(element("th"));

  const body = table.createTBody();
  for (const metric of section.metrics) {
    const row = body.insertRow();
    row.dataset.metric = shortName(metric.name);
    row.insertCell().textContent = shortName(metric.name);
    valueCell(row, metric.baseline, metric.unit);
    if (withTolerances) {
      const tolerance = row.insertCell();
      tolerance.className = "tolerance";
      if (metric.allowed_difference !== null) {
        tolerance.textContent = `±${formatValue(metric.allowed_difference, metric.unit)}`;
        tolerance.title = String(metric.allowed_difference);
      }
    }
    candidates.forEach((_, index) => {
      const { value, status } = metric.candidates[index] ?? { value: null, status: "missing" };
      if (status === "not compared") {
        const cell = row.insertCell();
        cell.textContent = "not compared";
        cell.colSpan = 2;
        cell.className = "missing";
        return;
      }
      const verdict = status === "passed" || status === "failed" ? ` ${status}` : "";
      valueCell(row, value, metric.unit).className += verdict;
      // clab reports the absolute difference; the sign shows which way the candidate moved.
      const delta = row.insertCell();
      const difference = value !== null && metric.baseline !== null ? value - metric.baseline : null;
      delta.textContent = difference === null ? "" : signed(difference, metric.unit);
      delta.className = `delta${verdict}`;
      delta.title = status;
    });
    row.insertCell().textContent = metric.unit;
  }

  const scroll = element("div", "", "table-scroll");
  scroll.append(table);
  return renderCard(section, { label: "Comparison", state }, scroll, report);
}
