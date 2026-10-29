import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { describe, run } from "./worker";

interface SignalInfo {
  name: string;
  unit: string;
}

interface Signals {
  axis: number[];
  signals: (SignalInfo & { values: number[] })[];
}

interface Series {
  label: string;
  values: number[];
}

interface Choice {
  value: string;
  text: string;
}

/** Selectable X/Y value that stands for the file's sample axis. */
const AXIS = "axis";

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

const labelOf = ({ name, unit }: SignalInfo) => (unit ? `${name} [${unit}]` : name);

/** Looks up what the user selected: the file's axis or one of the returned signals. */
export function seriesFor(data: Signals, name: string): Series {
  if (name === AXIS) return { label: AXIS, values: data.axis };
  const signal = data.signals.find((signal) => signal.name === name)!;
  return { label: labelOf(signal), values: signal.values };
}

function createSelect(options: Choice[], selected: string): HTMLSelectElement {
  const select = document.createElement("select");
  select.replaceChildren(...options.map(({ value, text }) => new Option(text, value)));
  select.value = selected;
  return select;
}

interface Curve {
  x: string;
  y: string;
  color: string;
}

async function drawChart(root: HTMLElement, data: Signals, curves: Curve[]): Promise<void> {
  // Plotly is large; load it on first plot only.
  const { default: Plotly } = await import("plotly.js-basic-dist-min");
  const text = getComputedStyle(document.body).color;
  const traces = curves.map(({ x, y, color }) => {
    const [xs, ys] = [seriesFor(data, x), seriesFor(data, y)];
    return {
      x: xs.values,
      y: ys.values,
      name: `${ys.label} vs ${xs.label}`,
      type: "scatter",
      mode: "lines",
      line: { color, width: 1.5 },
    };
  });
  // An axis title is only meaningful when every curve shares it.
  const common = (pick: (curve: Curve) => string) =>
    new Set(curves.map(pick)).size === 1 ? seriesFor(data, pick(curves[0])).label : "";
  const frame = { showline: true, mirror: true, linecolor: text, ticks: "outside" };
  const layout = {
    xaxis: { ...frame, title: { text: common((curve) => curve.x) } },
    yaxis: { ...frame, title: { text: common((curve) => curve.y) } },
    showlegend: curves.length > 1,
    legend: { orientation: "h", y: -0.2 },
    // Keep the tool the user picked across redraws.
    dragmode: document.querySelector<HTMLElement>("#inspect-tools [aria-pressed=true]")!.dataset.tool,
    margin: { t: 16 },
    paper_bgcolor: "transparent",
    plot_bgcolor: "transparent",
    font: { color: text },
  };
  await Plotly.newPlot(root, traces, layout, { responsive: true, displayModeBar: false });
}

/** Buttons beside the chart replace Plotly's own toolbar. */
function setupTools(
  chart: HTMLElement,
  tools: HTMLElement,
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
            "xaxis.linecolor": text,
            "yaxis.linecolor": text,
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
      await Plotly.relayout(chart, { "xaxis.autorange": true, "yaxis.autorange": true });
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
  const curvesRoot = document.querySelector<HTMLElement>("#curves")!;
  const addButton = document.querySelector<HTMLButtonElement>("#add-curve")!;
  const file = form.elements.namedItem("file") as HTMLInputElement;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  let choices: Choice[] = [];

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
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
      updateRemoveButtons();
    });

    row.append(swatch, x, y, visibleLabel, remove);
    curvesRoot.append(row);
    recolor();
  }

  const updateRemoveButtons = () =>
    rows().forEach((row) => {
      row.querySelector<HTMLButtonElement>("button")!.hidden = rows().length === 1;
    });

  /** Rows the user ticked, with the color their position gives them. */
  const selectedCurves = (): Curve[] =>
    rows().flatMap((row, index) => {
      const [x, y] = row.querySelectorAll("select");
      return row.querySelector<HTMLInputElement>("[name=show]")!.checked
        ? [{ x: x.value, y: y.value, color: colorFor(index) }]
        : [];
    });

  setupBrowse(form);
  setupTools(chart, document.querySelector<HTMLElement>("#inspect-tools")!, show);

  addButton.addEventListener("click", () => {
    addRow();
    updateRemoveButtons();
  });

  file.addEventListener("change", async () => {
    submit.disabled = true;
    addButton.disabled = true;
    show("Loading signals…");
    try {
      const { signals } = await inspect<{ signals: SignalInfo[] }>(file.value, []);
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
      await drawChart(chart, data, curves);
      footer.textContent = `File: ${file.value.trim()} · ${curves.length} curve${curves.length > 1 ? "s" : ""}`;
      show("");
    } catch (error) {
      show(String(error), false);
    } finally {
      submit.disabled = false;
    }
  });
}
