import { setupConvert } from "./convert";
import { setupInspect } from "./inspect";

const navItems = document.querySelectorAll<HTMLButtonElement>(".nav-item");
const panes = document.querySelectorAll<HTMLElement>(".pane");

function showPane(id: string): void {
  panes.forEach((pane) => (pane.hidden = pane.id !== id));
  navItems.forEach((item) => {
    if (item.dataset.pane === id) item.setAttribute("aria-current", "page");
    else item.removeAttribute("aria-current");
  });
}

navItems.forEach((item) => item.addEventListener("click", () => showPane(item.dataset.pane!)));

setupConvert();
setupInspect();
