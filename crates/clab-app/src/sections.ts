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
}

/** A foldable card, always closed at first; its badges show whether it needs a look. */
export function renderSection(section: Section, report?: ReportStatus): HTMLDetailsElement {
  const missing = section.metrics.some((metric) => metric.value === null);
  const state = missing ? "error" : section.warnings.length > 0 ? "warning" : "ok";

  const card = element("details", "", "section");
  const summary = element("summary", "");
  summary.append(element("span", section.title));
  if (report) summary.append(labeled("Report", report.badge));
  summary.append(labeled("Analysis", createBadge(state)));
  card.append(summary);

  const table = element("table", "", "metrics");
  for (const metric of section.metrics) {
    const row = table.insertRow();
    // Names arrive qualified (`svc::front_left_mass_kg`); the card title already says where.
    row.insertCell().textContent = metric.name.split("::").at(-1)!;
    const value = row.insertCell();
    value.textContent = formatValue(metric.value, metric.unit);
    value.className = metric.value === null ? "missing" : "num";
    if (metric.value !== null) value.title = String(metric.value);
    row.insertCell().textContent = metric.value === null ? "" : metric.unit;
  }
  card.append(table);

  if (section.warnings.length > 0) {
    const list = element("ul", "", "warnings");
    section.warnings.forEach((warning) => list.append(element("li", warning)));
    card.append(list);
  }
  if (report?.plots) card.append(element("p", `Plots: ${report.plots}`, "location"));
  return card;
}
