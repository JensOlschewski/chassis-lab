export interface SignalInfo {
  name: string;
  unit: string;
}

/** File facts from `clab inspect --json --list`; test metadata only exists for measurements. */
export interface Summary {
  source_file: string;
  source_kind: string;
  schema_version: number;
  samples: number;
  signals: number;
  test_metadata: number | null;
}

export interface Signals {
  axis: number[];
  signals: (SignalInfo & { values: number[] })[];
}

export interface Curve {
  x: string;
  y: string;
  panel: number;
  color: string;
}

interface Series {
  label: string;
  values: number[];
}

/** Selectable X/Y value that stands for the file's sample axis. */
export const AXIS = "axis";

const PANEL_HEIGHT = 200;
const BASE_HEIGHT = 480;
const PANEL_GAP = 0.06;

export const labelOf = ({ name, unit }: SignalInfo) => (unit ? `${name} [${unit}]` : name);

/** Looks up what the user selected: the file's axis or one of the returned signals. */
export function seriesFor(data: Signals, name: string): Series {
  if (name === AXIS) return { label: AXIS, values: data.axis };
  const signal = data.signals.find((signal) => signal.name === name)!;
  return { label: labelOf(signal), values: signal.values };
}

/** Plotly names the first axis `x`/`y` and the rest `x2`/`y2`, … */
const suffix = (index: number) => (index === 0 ? "" : String(index + 1));

/** Trace and layout for curves stacked in panels, lowest panel number on top. */
export function buildChart(data: Signals, curves: Curve[], text: string) {
  const panels = [...new Set(curves.map((curve) => curve.panel))].sort((a, b) => a - b);
  const indexOf = (curve: Curve) => panels.indexOf(curve.panel);
  const sharedX = new Set(curves.map((curve) => curve.x)).size === 1;

  const traces = curves.map((curve) => {
    const [xs, ys] = [seriesFor(data, curve.x), seriesFor(data, curve.y)];
    const s = suffix(indexOf(curve));
    return {
      x: xs.values,
      y: ys.values,
      name: `${ys.label} vs ${xs.label}`,
      type: "scatter",
      mode: "lines",
      line: { color: curve.color, width: 1.5 },
      xaxis: `x${s}`,
      yaxis: `y${s}`,
    };
  });

  // An axis title is only meaningful when every curve in the panel shares it.
  const common = (members: Curve[], pick: (curve: Curve) => string) =>
    new Set(members.map(pick)).size === 1 ? seriesFor(data, pick(members[0])).label : "";

  const frame = { showline: true, mirror: true, linecolor: text, ticks: "outside" };
  const height = (1 - PANEL_GAP * (panels.length - 1)) / panels.length;
  const axes: Record<string, object> = {};
  panels.forEach((panel, index) => {
    const members = curves.filter((curve) => curve.panel === panel);
    const s = suffix(index);
    const top = 1 - index * (height + PANEL_GAP);
    const last = index === panels.length - 1;
    // Linked X axes only label the bottom panel.
    const linked = sharedX && !last;
    axes[`yaxis${s}`] = {
      ...frame,
      domain: [top - height, top],
      anchor: `x${s}`,
      title: { text: common(members, (curve) => curve.y) },
    };
    axes[`xaxis${s}`] = {
      ...frame,
      anchor: `y${s}`,
      showticklabels: !linked,
      title: { text: linked ? "" : common(sharedX ? curves : members, (curve) => curve.x) },
      ...(sharedX && index > 0 ? { matches: "x" } : {}),
    };
  });

  const layout = {
    ...axes,
    showlegend: curves.length > 1,
    legend: { orientation: "h", y: -0.2 },
    margin: { t: 16 },
    paper_bgcolor: "transparent",
    plot_bgcolor: "transparent",
    font: { color: text },
  };
  return {
    traces,
    layout,
    axes: Object.keys(axes),
    pixels: BASE_HEIGHT + PANEL_HEIGHT * (panels.length - 1),
  };
}

/** Draws the chart and returns the layout keys of its axes. */
export async function drawChart(
  root: HTMLElement,
  data: Signals,
  curves: Curve[],
  dragmode: string | undefined,
): Promise<string[]> {
  // Plotly is large; load it on first plot only.
  const { default: Plotly } = await import("plotly.js-basic-dist-min");
  const { traces, layout, axes, pixels } = buildChart(data, curves, getComputedStyle(document.body).color);
  root.style.height = `${pixels}px`;
  await Plotly.newPlot(root, traces, { ...layout, dragmode }, { responsive: true, displayModeBar: false });
  return axes;
}
