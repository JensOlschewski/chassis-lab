import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { AXIS, Curve, SignalInfo, Signals, Summary, drawChart, labelOf } from "./chart";
import { describe, run } from "./worker";

interface Choice {
  value: string;
  text: string;
}

/** Same order as the Python reports: black baseline, then the compare palette. */
const CURVE_COLORS = ["#000000", "#e41a1c", "#377eb8", "#e7298a", "#00a6a6", "#ff7f00", "#6a3d9a"];
const colorFor = (index: number) => CURVE_COLORS[index % CURVE_COLORS.length];

async function inspect<T>(file: string, signals: string[]): Promise<T> {
  const outcome = await run("run_inspect", { file, signals });
  if (outcome.exit_code !== 0) {
    throw new Error(outcome.stderr.trim() || describe(outcome).message);
  }
  return JSON.parse(outcome.stdout) as T;
}

function createSelect(options: Choice[], selected: string): HTMLSelectElement {
  const select = document.createElement("select");
  select.replaceChildren(...options.map(({ value, text }) => new Option(text, value)));
  select.value = selected;
  return select;
}

/** Buttons beside the chart replace Plotly's own toolbar. */
function setupTools(
  chart: HTMLElement,
  tools: HTMLElement,
  axes: () => string[],
  report: (message: string, ok?: boolean) => void,
): void {
  const buttons = [...tools.querySelectorAll<HTMLButtonElement>("button")];
  tools.addEventListener("click", async (event) => {
    const button = (event.target as HTMLElement).closest("button");
    if (!button) return;
    const { default: Plotly } = await import("plotly.js-basic-dist-min");
    const tool = button.dataset.tool!;
    if (tool === "export") {
      try {
        const path = await save({ defaultPath: "plot.png", filters: [{ name: "PNG", extensions: ["png"] }] });
        if (!path) return;
        // Export on white with dark text, whatever the app theme is.
        const paint = (background: string, text: string) =>
          Plotly.relayout(chart, {
            paper_bgcolor: background,
            plot_bgcolor: background,
            "font.color": text,
            ...Object.fromEntries(axes().map((axis) => [`${axis}.linecolor`, text])),
          });
        await paint("white", "black");
        let image: string;
        try {
          image = await Plotly.toImage(chart, { format: "png", scale: 2 });
        } finally {
          await paint("transparent", getComputedStyle(document.body).color);
        }
        const bytes = new Uint8Array(await (await fetch(image)).arrayBuffer());
        await invoke("save_image", { path, bytes: Array.from(bytes) });
        report(`Saved ${path}`);
      } catch (error) {
        report(String(error), false);
      }
      return;
    }
    if (tool === "fit") {
      await Plotly.relayout(chart, Object.fromEntries(axes().map((axis) => [`${axis}.autorange`, true])));
      return;
    }
    await Plotly.relayout(chart, { dragmode: tool });
    buttons.forEach((b) => b.setAttribute("aria-pressed", String(b === button)));
  });
}

export function setupInspect(): void {
  const form = document.querySelector<HTMLFormElement>("#inspect-form")!;
  const status = document.querySelector<HTMLElement>("#inspect-status")!;
  const chart = document.querySelector<HTMLElement>("#inspect-chart")!;
  const plot = document.querySelector<HTMLElement>("#inspect-plot")!;
  const footer = document.querySelector<HTMLElement>("#inspect-footer")!;
  const info = document.querySelector<HTMLElement>("#inspect-info")!;
  const curvesRoot = document.querySelector<HTMLElement>("#curves")!;
  const addButton = document.querySelector<HTMLButtonElement>("#add-curve")!;
  const file = form.elements.namedItem("file") as HTMLInputElement;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  let choices: Choice[] = [];
  let axes: string[] = [];

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const showInfo = (summary: Summary) => {
    const fields: [string, string | number][] = [
      ["Source", summary.source_file],
      ["Kind", summary.source_kind],
      ["Samples", summary.samples],
      ["Signals", summary.signals],
    ];
    info.replaceChildren(
      ...fields.flatMap(([label, value]) => {
        const [dt, dd] = [document.createElement("dt"), document.createElement("dd")];
        dt.textContent = label;
        dd.textContent = String(value);
        return [dt, dd];
      }),
    );
    info.hidden = false;
  };

  const rows = () => [...curvesRoot.querySelectorAll<HTMLElement>(".curve")];

  /** Keeps swatch colors in step with row order after adding or removing rows. */
  const recolor = () =>
    rows().forEach((row, index) => {
      row.querySelector<HTMLElement>(".swatch")!.style.background = colorFor(index);
    });

  function addRow(): void {
    const row = document.createElement("div");
    row.className = "curve";

    const swatch = document.createElement("span");
    swatch.className = "swatch";
    const x = createSelect(choices, choices.some((c) => c.value === "time") ? "time" : AXIS);
    const y = createSelect(choices, choices[1].value);
    x.name = "x";
    y.name = "y";

    const visible = document.createElement("input");
    visible.type = "checkbox";
    visible.name = "show";
    visible.checked = true;
    const visibleLabel = document.createElement("label");
    visibleLabel.className = "check";
    visibleLabel.append(visible, "Plot");

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "×";
    remove.ariaLabel = "Remove curve";
    remove.addEventListener("click", () => {
      row.remove();
      recolor();
      syncPanels();
      updateRemoveButtons();
    });

    const panel = createSelect([], "1");
    panel.name = "panel";
    panel.ariaLabel = "Panel";

    row.append(swatch, x, y, panel, visibleLabel, remove);
    curvesRoot.append(row);
    recolor();
    syncPanels();
  }

  /** Offers panels 1..N for N rows, keeping each choice that is still valid. */
  const syncPanels = () => {
    const options = rows().map((_, index) => ({ value: String(index + 1), text: `Panel ${index + 1}` }));
    rows().forEach((row) => {
      const select = row.querySelector<HTMLSelectElement>("[name=panel]")!;
      const kept = Math.min(Number(select.value) || 1, options.length);
      select.replaceChildren(...options.map(({ value, text }) => new Option(text, value)));
      select.value = String(kept);
    });
  };

  const updateRemoveButtons = () =>
    rows().forEach((row) => {
      row.querySelector<HTMLButtonElement>("button")!.hidden = rows().length === 1;
    });

  /** Rows the user ticked, with the color their position gives them. */
  const selectedCurves = (): Curve[] =>
    rows().flatMap((row, index) => {
      const [x, y, panel] = row.querySelectorAll("select");
      return row.querySelector<HTMLInputElement>("[name=show]")!.checked
        ? [{ x: x.value, y: y.value, panel: Number(panel.value), color: colorFor(index) }]
        : [];
    });

  setupBrowse(form);
  setupTools(chart, document.querySelector<HTMLElement>("#inspect-tools")!, () => axes, show);

  addButton.addEventListener("click", () => {
    addRow();
    updateRemoveButtons();
  });

  file.addEventListener("change", async () => {
    submit.disabled = true;
    addButton.disabled = true;
    info.hidden = true;
    show("Loading signals…");
    try {
      const { summary, signals } = await inspect<{ summary: Summary; signals: SignalInfo[] }>(file.value, []);
      showInfo(summary);
      const sorted = [...signals]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((signal) => ({ value: signal.name, text: labelOf(signal) }));
      choices = [{ value: AXIS, text: AXIS }, ...sorted];
      curvesRoot.replaceChildren();
      addRow();
      updateRemoveButtons();
      submit.disabled = false;
      addButton.disabled = false;
      show(`${signals.length} signals`);
    } catch (error) {
      show(String(error), false);
    }
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const curves = selectedCurves();
    if (curves.length === 0) return show("Tick at least one curve", false);
    submit.disabled = true;
    show("Plotting…");
    try {
      const names = new Set(curves.flatMap(({ x, y }) => [x, y]));
      names.delete(AXIS);
      const data = await inspect<Signals>(file.value, [...names]);
      plot.hidden = false;
      const pressed = document.querySelector<HTMLElement>("#inspect-tools [aria-pressed=true]");
      // Keep the tool the user picked across redraws.
      axes = await drawChart(chart, data, curves, pressed?.dataset.tool);
      footer.textContent = `File: ${file.value.trim()} · ${curves.length} curve${curves.length > 1 ? "s" : ""}`;
      show("");
    } catch (error) {
      show(String(error), false);
    } finally {
      submit.disabled = false;
    }
  });
}
