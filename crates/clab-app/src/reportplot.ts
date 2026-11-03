/** One plot of `knc-<test>.plots.json`, as written by the Python plot kernel (schema_version 1). */
export interface PlotSource {
  label: string;
  color: string;
  x: (number | null)[];
  y: (number | null)[];
  /** Plotted values at -half and +half; null when the secant is not defined. */
  secant: [number, number] | null;
  metric: number | null;
}

export interface PlotPanel {
  corner: string;
  title: string;
  x_label: string;
  y_label: string;
  sources: PlotSource[];
}

export interface Gradient {
  label: string;
  unit: string;
  half: number;
  half_unit: string;
  axis_label: string;
}

export interface Plot {
  name: string;
  title: string;
  y_limits: [number, number] | null;
  footer: string;
  gradient: Gradient | null;
  panels: PlotPanel[];
}

export interface PlotsFile {
  schema_version: number;
  test: string;
  plots: Plot[];
}

// Layout of the PDF report page (A4 landscape), mirrored from the Python plot kernel.
const PAGE = { width: 11.69, height: 8.27 };
const GRID = { left: 0.08, right: 0.97, top: 0.89, wspace: 0.28, hspace: 0.42 };
const FOOTER = { base: 0.12, perLine: 0.025, max: 0.35, fontPt: 7, color: "#4a4a4a", logoPerLine: 0.017, logoMargin: 0.05 };
const TABLE = { fontPt: 8, rowInches: 0.185, gapInches: 0.08, xlabelInches: 0.62, left: 0.02, color: "#222222" };
const SECANT = { width: 0.9, markerColor: "#d62728", markerPt: 5, tickPt: 12, markerWidth: 1.2 };
const LOGO_SLOT = { x: 0.8, width: 0.18 };
const AVERAGE_CHAR_EM = 0.55;

const CORNER_LABELS: Record<string, string> = {
  front_left: "Left front",
  front_right: "Right front",
  rear_left: "Left rear",
  rear_right: "Right rear",
};

const escapeHtml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Decimals that give four significant digits to the smallest nonzero value. */
function metricDecimals(values: number[]): number {
  const nonzero = values.filter((value) => value !== 0).map(Math.abs);
  if (nonzero.length === 0) return 0;
  return Math.min(Math.max(0, 3 - Math.floor(Math.log10(Math.min(...nonzero)))), 8);
}

const signed = (value: number) => `${value < 0 ? "-" : "+"}${Number(Math.abs(value).toPrecision(6))}`;

const suffix = (index: number) => (index === 0 ? "" : String(index + 1));

/** Domains `[x0, x1, y0, y1]` per panel: a 2×2 grid, or a wide top panel over two for axle plots. */
function panelDomains(count: number, bottom: number): [number, number, number, number][] {
  const { left, right, top, wspace, hspace } = GRID;
  const width = (right - left) / (2 + wspace);
  const height = (top - bottom) / (2 + hspace);
  const column = (index: number) => left + index * width * (1 + wspace);
  const row = (index: number): [number, number] => {
    const upper = top - index * height * (1 + hspace);
    return [upper - height, upper];
  };
  if (count === 3) {
    return [[left, right, ...row(0)], [column(0), column(0) + width, ...row(1)], [column(1), column(1) + width, ...row(1)]];
  }
  return Array.from({ length: count }, (_, index) => [
    column(index % 2),
    column(index % 2) + width,
    ...row(Math.floor(index / 2)),
  ]);
}

/**
 * Traces and layout that look like the PDF page: title, four panels, secant, the metric table
 * above the footer text, and the logo. `width` is the pixel width; the height follows the page.
 */
export function buildReportPlot(plot: Plot, width: number, logo: string) {
  const inch = width / PAGE.width;
  const height = PAGE.height * inch;
  const pt = inch / 72;
  const lines = plot.footer ? plot.footer.split("\n").length + 1 : 0;

  const band = plot.footer ? Math.min(FOOTER.base + lines * FOOTER.perLine, FOOTER.max) : 0;
  const logoHeight = plot.footer ? Math.min(lines * FOOTER.logoPerLine, band - FOOTER.logoMargin) : 0;
  const textInches = (lines * FOOTER.fontPt * 1.2) / 72;
  const baseInches = 0.025 * PAGE.height;
  const rows = plot.gradient ? plot.panels.length + 1 : 0;
  const tableBottom =
    Math.max(baseInches + textInches, baseInches + logoHeight * PAGE.height) + (rows ? TABLE.gapInches : 0);
  const tableTop = tableBottom + rows * TABLE.rowInches;
  const bottom = plot.footer
    ? Math.max(band, (tableTop + (rows ? TABLE.xlabelInches : 0)) / PAGE.height)
    : 0.1;

  const domains = panelDomains(plot.panels.length, bottom);
  const traces: object[] = [];
  const layout: Record<string, unknown> = {};
  const annotations: object[] = [];
  const axes: string[] = [];
  const frame = { showline: true, mirror: true, linecolor: "#000", ticks: "outside", zeroline: false, showgrid: false };

  plot.panels.forEach((panel, index) => {
    const s = suffix(index);
    const [x0, x1, y0, y1] = domains[index];
    layout[`xaxis${s}`] = {
      ...frame,
      domain: [x0, x1],
      anchor: `y${s}`,
      title: { text: panel.x_label, font: { size: 10 * pt } },
    };
    layout[`yaxis${s}`] = {
      ...frame,
      domain: [y0, y1],
      anchor: `x${s}`,
      title: { text: panel.y_label, font: { size: 10 * pt } },
      ...(plot.y_limits ? { range: plot.y_limits } : {}),
    };
    axes.push(`xaxis${s}`, `yaxis${s}`);
    annotations.push({
      xref: "paper", yref: "paper", x: x0, y: y1, xanchor: "left", yanchor: "bottom",
      showarrow: false, text: `<b>${escapeHtml(panel.title)}</b>`, font: { size: 12 * pt },
    });

    for (const source of panel.sources) {
      const common = { xaxis: `x${s}`, yaxis: `y${s}`, legendgroup: source.label };
      traces.push({
        ...common, type: "scatter", mode: "lines", x: source.x, y: source.y, name: source.label,
        showlegend: index === 0, line: { color: source.color, width: 1.8 * pt },
      });
      if (!source.secant || !plot.gradient) continue;
      const { half } = plot.gradient;
      const xs = source.x.filter((value): value is number => value !== null);
      const slope = (source.secant[1] - source.secant[0]) / (2 * half);
      const ends = [Math.min(...xs), Math.max(...xs)];
      const points = [-half, half];
      traces.push({
        ...common, type: "scatter", mode: "lines", hoverinfo: "skip", showlegend: false,
        x: ends, y: ends.map((x) => source.secant![0] + slope * (x + half)),
        line: { color: source.color, width: SECANT.width * pt, dash: "dash" },
      });
      for (const [symbol, size] of [["circle-open", SECANT.markerPt], ["line-ns-open", SECANT.tickPt]] as const) {
        traces.push({
          ...common, type: "scatter", mode: "markers", hoverinfo: "skip", showlegend: false,
          x: points, y: source.secant,
          marker: { symbol, size: size * pt, color: SECANT.markerColor, line: { color: SECANT.markerColor, width: SECANT.markerWidth * pt } },
        });
      }
    }
  });

  if (plot.footer) {
    annotations.push({
      xref: "paper", yref: "paper", x: TABLE.left, y: 0.025, xanchor: "left", yanchor: "bottom", align: "left",
      showarrow: false, text: plot.footer.split("\n").map(escapeHtml).join("<br>"),
      font: { size: FOOTER.fontPt * pt, color: FOOTER.color },
    });
  }
  if (plot.gradient) annotations.push(...metricTable(plot, plot.gradient, tableTop / PAGE.height, pt));

  return {
    traces,
    axes,
    layout: {
      ...layout,
      width,
      height,
      margin: { l: 0, r: 0, t: 0, b: 0 },
      paper_bgcolor: "#fff",
      plot_bgcolor: "#fff",
      font: { color: "#000", size: 10 * pt },
      title: { text: escapeHtml(plot.title), x: 0.5, y: 0.99, xref: "paper", yref: "paper", xanchor: "center", yanchor: "top", font: { size: 16 * pt } },
      legend: { orientation: "h", x: GRID.right, xanchor: "right", y: GRID.top + 0.005, yanchor: "bottom", font: { size: 10 * pt } },
      annotations,
      images: plot.footer
        ? [{ source: logo, xref: "paper", yref: "paper", x: LOGO_SLOT.x, y: 0.025, sizex: LOGO_SLOT.width, sizey: logoHeight, xanchor: "left", yanchor: "bottom", sizing: "contain", layer: "above" }]
        : [],
    },
  };
}

/** Header and one row per corner: the source name in its plot color, then the metric. */
function metricTable(plot: Plot, gradient: Gradient, top: number, pt: number): object[] {
  const decimals = metricDecimals(
    plot.panels.flatMap((panel) => panel.sources.flatMap((source) => (source.metric === null ? [] : [source.metric]))),
  );
  const row = TABLE.rowInches / PAGE.height;
  const char = (AVERAGE_CHAR_EM * TABLE.fontPt) / 72 / PAGE.width;
  const cell = (text: string, x: number, y: number) => ({
    xref: "paper", yref: "paper", x, y, xanchor: "left", yanchor: "top", align: "left", showarrow: false,
    text, font: { size: TABLE.fontPt * pt, color: TABLE.color },
  });
  const title = `${gradient.label.charAt(0).toUpperCase()}${gradient.label.slice(1)}, secant between ${signed(-gradient.half)} ${gradient.half_unit} and ${signed(gradient.half)} ${gradient.half_unit} ${gradient.axis_label.toLowerCase()}`;
  const labels = plot.panels.map((panel) => `${CORNER_LABELS[panel.corner] ?? panel.title} ${gradient.label} in ${gradient.unit}:`);
  const values = plot.panels.map((panel) =>
    panel.sources.map((source) => (source.metric === null ? "n/a" : source.metric.toFixed(decimals))),
  );
  const start = TABLE.left + Math.max(...labels.map((label) => label.length)) * char + 0.02;
  const sources = plot.panels[0].sources;
  const steps = sources.map(
    (source, column) => Math.max(...values.map((line) => source.label.length + 1 + line[column].length)) * char * 1.05 + 0.025,
  );
  const cells: object[] = [cell(`<b>${escapeHtml(title)}</b>`, TABLE.left, top)];
  labels.forEach((label, index) => {
    const y = top - (index + 1) * row;
    cells.push(cell(escapeHtml(label), TABLE.left, y));
    let x = start;
    sources.forEach((source, column) => {
      cells.push(cell(`<b><span style="color:${source.color}">${escapeHtml(source.label)}</span> ${values[index][column]}</b>`, x, y));
      x += steps[column];
    });
  });
  return cells;
}

/** Draws one plot sized to `root`'s width; returns the layout keys of its axes. */
export async function drawReportPlot(root: HTMLElement, plot: Plot, logo: string, dragmode?: string): Promise<string[]> {
  const { default: Plotly } = await import("plotly.js-basic-dist-min");
  const { traces, layout, axes } = buildReportPlot(plot, Math.max(root.clientWidth, 600), logo);
  root.style.height = `${layout.height}px`;
  await Plotly.newPlot(root, traces, { ...layout, dragmode }, { responsive: false, displayModeBar: false });
  return axes;
}
