import { invoke } from "@tauri-apps/api/core";
import { setupBrowse } from "./browse";
import { colorFor } from "./colors";
import { AXIS, Curve, SignalInfo, Signals, Summary, drawChart, labelOf, setupTools } from "./chart";
import { setupReport } from "./report";
import type { Viewer, ViewFrame } from "./viewer";
import { describe, errorLine, run } from "./worker";

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


/** `clab inspect --json` of a solver solution; HDF5 listings have no `kind`. */
interface SolutionResult {
  kind: "solution";
  summary: string;
  /** Body velocities, ready to plot. */
  signals: Signals;
  /** Null unless solved with `--include bodies`. */
  frames: ViewFrame[] | null;
}

const isSolution = (result: object): result is SolutionResult => "kind" in result && result.kind === "solution";

async function inspect<T>(file: string, signals: string[], view?: string): Promise<T> {
  const outcome = await run("run_inspect", { file, signals, view });
  if (outcome.exit_code !== 0) {
    throw new Error(errorLine(outcome.stderr) ?? describe(outcome).message);
  }
  return JSON.parse(outcome.stdout) as T;
}

function createSelect(options: Choice[], selected: string): HTMLSelectElement {
  const select = document.createElement("select");
  select.replaceChildren(...options.map(({ value, text }) => new Option(text, value)));
  select.value = selected;
  return select;
}

export function setupInspect() {
  const pathForm = document.querySelector<HTMLFormElement>("#inspect-form")!;
  const form = document.querySelector<HTMLFormElement>("#inspect-signals")!;
  const reportRoot = document.querySelector<HTMLElement>("#inspect-report")!;
  const status = document.querySelector<HTMLElement>("#inspect-status")!;
  const chart = document.querySelector<HTMLElement>("#inspect-chart")!;
  const plot = document.querySelector<HTMLElement>("#inspect-plot")!;
  const footer = document.querySelector<HTMLElement>("#inspect-footer")!;
  const info = document.querySelector<HTMLElement>("#inspect-info")!;
  const summaryText = document.querySelector<HTMLElement>("#inspect-summary")!;
  const viewRoot = document.querySelector<HTMLElement>("#inspect-view")!;
  const viewBar = document.querySelector<HTMLElement>("#inspect-viewbar")!;
  const viewOn = document.querySelector<HTMLButtonElement>("#inspect-view-on")!;
  const viewShown = () => viewOn.getAttribute("aria-pressed") === "true";
  const layout = form.elements.namedItem("view") as HTMLInputElement;
  const curvesRoot = document.querySelector<HTMLElement>("#curves")!;
  const addButton = document.querySelector<HTMLButtonElement>("#add-curve")!;
  const file = pathForm.elements.namedItem("file") as HTMLInputElement;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  let choices: Choice[] = [];
  let axes: string[] = [];
  let plotted = false;
  /** A solver solution carries its signals with the listing; HDF5 signals are read per plot. */
  let solution: Signals | null = null;
  let viewer: Viewer | null = null;
  let scene: ViewFrame[] | null = null;
  const bodyRoot = document.querySelector<HTMLElement>("#inspect-body-plot")!;
  let bodyToken = 0;
  let bodyReady = false;
  let cursorIndex = 0;

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const report = setupReport(show);

  /** Only one body shows at a time: signals form and chart, or the report. */
  const showBody = (body: "signals" | "report" | "none") => {
    form.hidden = body !== "signals";
    reportRoot.hidden = body !== "report";
  };

  const showInfo = (summary: Summary) => {
    const fields: [string, string | number][] = [
      ["Source", summary.source_file],
      ["Kind", summary.source_kind],
      ...(summary.setup ? [["Setup", summary.setup] as [string, string]] : []),
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

  setupBrowse(pathForm);
  setupBrowse(form);
  setupTools(chart, document.querySelector<HTMLElement>("#inspect-tools")!, () => axes, show);

  addButton.addEventListener("click", () => {
    addRow();
    updateRemoveButtons();
    changed();
  });

  /** Drops the previous file's rows, or report, and disables plotting until signals load. */
  const clear = () => {
    report.clear();
    plotted = false;
    plot.hidden = true;
    submit.disabled = true;
    addButton.disabled = true;
    info.hidden = true;
    summaryText.hidden = true;
    drawScene(null);
    viewBar.hidden = true;
    layout.value = "";
    viewOn.setAttribute("aria-pressed", "true");
    solution = null;
    curvesRoot.replaceChildren();
  };

  /** Replaces the 3D view; `null`, or the Viewer button off, leaves none. */
  async function drawScene(frames: ViewFrame[] | null): Promise<void> {
    viewer?.destroy();
    viewer = null;
    plotBody(null);
    scene = frames;
    viewRoot.hidden = !frames || !viewShown();
    if (viewRoot.hidden) return;
    const { createViewer } = await import("./viewer");
    viewer = createViewer(viewRoot, frames!, plotBody, moveCursor);
  }

  /** Velocities of the selected body, with a cursor that follows the viewer's time step. */
  async function plotBody(body: number | null): Promise<void> {
    const token = ++bodyToken;
    bodyReady = false;
    bodyRoot.hidden = true;
    const label = body === null ? null : scene?.[0].bodies[body].label;
    if (!label || !solution) return;
    const has = (name: string) => solution!.signals.some((signal) => signal.name === name);
    const curves: Curve[] = (["vx", "vy", "vz", "wx", "wy", "wz"] as const)
      .map((component, index) => ({ x: AXIS, y: `${label}.${component}`, panel: index < 3 ? 1 : 2, color: colorFor(index) }))
      .filter((curve) => has(curve.y));
    if (!curves.length) return;
    bodyRoot.hidden = false;
    await drawChart(bodyRoot, solution, curves, undefined);
    if (token !== bodyToken) return;
    bodyReady = true;
    moveCursor(cursorIndex);
  }

  async function moveCursor(index: number): Promise<void> {
    cursorIndex = index;
    if (!bodyReady || !solution) return;
    const x = solution.axis[index];
    const { default: Plotly } = await import("plotly.js-basic-dist-min");
    if (!bodyReady) return;
    const line = { color: getComputedStyle(document.body).color, width: 1, dash: "dot" };
    await Plotly.relayout(bodyRoot, { shapes: [{ type: "line", xref: "x", yref: "paper", x0: x, x1: x, y0: 0, y1: 1, line }] });
  }

  viewOn.addEventListener("click", () => {
    viewOn.setAttribute("aria-pressed", String(!viewShown()));
    drawScene(scene);
  });
  layout.addEventListener("change", async () => {
    try {
      const result = await inspect<SolutionResult>(file.value, [], layout.value.trim() || undefined);
      await drawScene(result.frames);
      show("");
    } catch (error) {
      show(String(error), false);
    }
  });

  /** Lists the file's signals and starts with one default row; false if that failed. */
  async function loadSignals(): Promise<boolean> {
    clear();
    showBody("signals");
    show("Loading signals…");
    try {
      const result = await inspect<SolutionResult | { summary: Summary; signals: SignalInfo[] }>(file.value, []);
      let signals: SignalInfo[];
      if (isSolution(result)) {
        solution = result.signals;
        signals = solution.signals;
        summaryText.textContent = result.summary;
        summaryText.hidden = false;
        viewBar.hidden = !result.frames;
        await drawScene(result.frames);
      } else {
        signals = result.signals;
        showInfo(result.summary);
      }
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

  /** Opens whatever the field points at: a signals file or a report folder. */
  async function open(test?: string): Promise<"signals" | "report" | undefined> {
    clear();
    showBody("none");
    show("");
    const kind = await invoke<string>("path_kind", { path: file.value });
    if (kind === "signals") return (await loadSignals()) ? "signals" : undefined;
    else if (kind === "report") {
      showBody("report");
      await report.open(file.value, test);
      return "report";
    } else show(kind === "folder" ? "Not a report folder: report.json is missing" : "Not found", false);
  }

  file.addEventListener("change", async () => {
    await open();
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
      const data = solution ?? (await inspect<Signals>(file.value, [...names]));
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
    /** Shows a report folder (or signals file) from elsewhere in the app; the pane must be visible. */
    async show(path: string, test?: string): Promise<void> {
      file.value = path;
      await open(test);
      changed();
    },
    get: (): InspectState => ({ file: file.value.trim() || null, curves: savedCurves(), plotted }),
    /** A file that fails to load keeps its path; the error is shown as for a manual pick. */
    async set(state: InspectState): Promise<void> {
      file.value = state.file ?? "";
      footer.textContent = "";
      if (!state.file) {
        clear();
        showBody("none");
        return show("");
      }
      if ((await open()) !== "signals" || state.curves.length === 0) return;

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
