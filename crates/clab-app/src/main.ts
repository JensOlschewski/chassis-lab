import { setupAnalyze } from "./analyze";
import { setupConvert } from "./convert";
import { setupInspect } from "./inspect";
import { setupOptimize } from "./optimize";
import { setupSession } from "./session";
import { setupSolve } from "./solve";

const navItems = document.querySelectorAll<HTMLButtonElement>(".nav-item[data-pane]");
const panes = document.querySelectorAll<HTMLElement>(".pane");

function showPane(id: string): void {
  panes.forEach((pane) => (pane.hidden = pane.id !== id));
  navItems.forEach((item) => {
    if (item.dataset.pane === id) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  });
}

navItems.forEach((item) => item.addEventListener("click", () => showPane(item.dataset.pane!)));

const inspect = setupInspect();
const analyze = setupAnalyze((directory, test) => {
  // Plotly needs a visible container to size the chart.
  showPane("inspect");
  void inspect.show(directory, test);
});
const openModel = setupSolve((path) => {
  showPane("inspect");
  void inspect.show(path);
});
setupOptimize((path) => {
  showPane("solve");
  void openModel(path);
});
setupSession(setupConvert(), inspect, analyze);
