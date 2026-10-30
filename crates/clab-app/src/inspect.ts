import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { AXIS, Curve, SignalInfo, Signals, Summary, drawChart, labelOf } from "./chart";
import { describe, run } from "./worker";

interface Choice {
  value: string;
  text: string;
}

/** A curve row as stored in a session file. */
interface SavedCurve {
  x: string;
  y: string;
  panel: number;
  plot: boolean;
}

/** The form as stored in a session file. */
export interface InspectState {
  file: string | null;
  curves: SavedCurve[];
  /** Whether the chart was drawn; reopening redraws it. */
  plotted: boolean;
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

export function setupInspect() {
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
  let plotted = false;

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

  /** Tells the session that the curve rows changed; row selects report their own changes. */
  const changed = () => curvesRoot.dispatchEvent(new Event("change", { bubbles: true }));

  const pick = (wanted: string | undefined, fallback: string) =>
    choices.some((choice) => choice.value === wanted) ? wanted! : fallback;

  function addRow(saved?: SavedCurve): void {
    const row = document.createElement("div");
    row.className = "curve";

    const swatch = document.createElement("span");
    swatch.className = "swatch";
    const x = createSelect(choices, pick(saved?.x, choices.some((c) => c.value === "time") ? "time" : AXIS));
    const y = createSelect(choices, pick(saved?.y, choices[1].value));
    x.name = "x";
    y.name = "y";

    const visible = document.createElement("input");
    visible.type = "checkbox";
    visible.name = "show";
    visible.checked = saved?.plot ?? true;
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
      changed();
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

  const savedCurves = (): SavedCurve[] =>
    rows().map((row) => {
      const [x, y, panel] = row.querySelectorAll("select");
      return {
        x: x.value,
        y: y.value,
        panel: Number(panel.value),
        plot: row.querySelector<HTMLInputElement>("[name=show]")!.checked,
      };
    });

  /** Rows the user ticked, with the color their position gives them. */
  const selectedCurves = (): Curve[] =>
    savedCurves().flatMap(({ x, y, panel, plot }, index) =>
      plot ? [{ x, y, panel, color: colorFor(index) }] : [],
    );

  setupBrowse(form);
  setupTools(chart, document.querySelector<HTMLElement>("#inspect-tools")!, () => axes, show);

  addButton.addEventListener("click", () => {
    addRow();
    updateRemoveButtons();
    changed();
  });

  /** Drops the previous file's rows and disables plotting until signals load. */
  const clear = () => {
    plotted = false;
    plot.hidden = true;
    submit.disabled = true;
    addButton.disabled = true;
    info.hidden = true;
    curvesRoot.replaceChildren();
  };

  /** Lists the file's signals and starts with one default row; false if that failed. */
  async function loadSignals(): Promise<boolean> {
    clear();
    show("Loading signals…");
    try {
      const { summary, signals } = await inspect<{ summary: Summary; signals: SignalInfo[] }>(file.value, []);
      showInfo(summary);
      const sorted = [...signals]
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((signal) => ({ value: signal.name, text: labelOf(signal) }));
      choices = [{ value: AXIS, text: AXIS }, ...sorted];
      addRow();
      updateRemoveButtons();
      submit.disabled = false;
      addButton.disabled = false;
      show(`${signals.length} signals`);
      return true;
    } catch (error) {
      show(String(error), false);
      return false;
    }
  }

  file.addEventListener("change", async () => {
    await loadSignals();
    changed();
  });

  async function plotCurves(): Promise<void> {
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
      plotted = true;
      changed();
    } catch (error) {
      show(String(error), false);
    } finally {
      submit.disabled = false;
    }
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    return plotCurves();
  });

  return {
    get: (): InspectState => ({ file: file.value.trim() || null, curves: savedCurves(), plotted }),
    /** A file that fails to load keeps its path; the error is shown as for a manual pick. */
    async set(state: InspectState): Promise<void> {
      file.value = state.file ?? "";
      footer.textContent = "";
      if (!state.file) {
        clear();
        return show("");
      }
      if (!(await loadSignals()) || state.curves.length === 0) return;

      curvesRoot.replaceChildren();
      state.curves.forEach((curve) => addRow(curve));
      rows().forEach((row, index) => {
        const panel = row.querySelector<HTMLSelectElement>("[name=panel]")!;
        panel.value = String(Math.min(Math.max(state.curves[index].panel, 1), rows().length));
      });
      updateRemoveButtons();
      if (state.plotted) await plotCurves();
    },
  };
}
