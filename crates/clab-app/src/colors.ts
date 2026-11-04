/** Same order as the Python reports: black baseline, then the compare palette. */
const CURVE_COLORS = ["#000000", "#e41a1c", "#377eb8", "#e7298a", "#00a6a6", "#ff7f00", "#6a3d9a"];

/** Color of the `index`th series; 0 is the baseline. */
export const colorFor = (index: number) => CURVE_COLORS[index % CURVE_COLORS.length];

/** A round color marker for a series. */
export function swatch(index: number): HTMLSpanElement {
  const marker = document.createElement("span");
  marker.className = "swatch";
  marker.style.background = colorFor(index);
  return marker;
}
