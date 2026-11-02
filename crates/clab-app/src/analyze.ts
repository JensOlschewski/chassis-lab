import { setupBrowse } from "./browse";
import { formatValue } from "./format";
import { describe, errorLine, run } from "./worker";

interface Metric {
  name: string;
  unit: string;
  value: number | null;
}

interface Section {
  title: string;
  metrics: Metric[];
  warnings: string[];
}

/** Parses the `--json` stdout: one `{"section": …}` line per section. */
export function parseSections(stdout: string): Section[] {
  return stdout
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line).section as Section | undefined)
    .filter((section): section is Section => section !== undefined);
}

const element = <K extends keyof HTMLElementTagNameMap>(tag: K, text = "", className = "") => {
  const node = document.createElement(tag);
  node.textContent = text;
  node.className = className;
  return node;
};

const BADGE_GLYPH = { ok: "✓", warning: "!", error: "✕" };

/** A foldable card; sections that need a look (warnings, missing values) start open. */
function renderSection(section: Section): HTMLDetailsElement {
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

export function setupAnalyze() {
  const form = document.querySelector<HTMLFormElement>("#analyze-form")!;
  const status = document.querySelector<HTMLElement>("#analyze-status")!;
  const sections = document.querySelector<HTMLElement>("#analyze-sections")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  const show = (message: string, ok: boolean) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  setupBrowse(form);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    sections.replaceChildren();
    show("Analyzing…", true);
    try {
      const data = new FormData(form);
      const text = (name: string) => String(data.get(name) ?? "").trim();
      const outcome = await run("run_analyze", {
        config: text("config"),
        input: text("input"),
        report: text("report") || null,
        force: data.has("force"),
      });
      const { ok, message } = describe(outcome);
      if (!ok) {
        show(errorLine(outcome.stderr) ?? message, false);
        return;
      }
      sections.replaceChildren(...parseSections(outcome.stdout).map(renderSection));
      show(message, true);
    } catch (error) {
      show(`Failed to run clab: ${error}`, false);
    } finally {
      submit.disabled = false;
    }
  });
}
