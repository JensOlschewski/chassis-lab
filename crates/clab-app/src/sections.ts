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
}

export const element = <K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className = "") => {
  const node = document.createElement(tag);
  node.textContent = text;
  node.className = className;
  return node;
};

const BADGE_GLYPH = { ok: "✓", warning: "!", error: "✕" };

/** A foldable card; sections that need a look (warnings, missing values) start open. */
export function renderSection(section: Section): HTMLDetailsElement {
  const missing = section.metrics.some((metric) => metric.value === null);
  const state = missing ? "error" : section.warnings.length > 0 ? "warning" : "ok";

  const card = element("details", "", "section");
  card.open = state !== "ok";
  const summary = element("summary", "");
  summary.append(element("span", section.title), element("span", BADGE_GLYPH[state], `badge ${state}`));
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
  return card;
}
