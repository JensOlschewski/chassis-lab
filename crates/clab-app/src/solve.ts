import { Channel, invoke } from "@tauri-apps/api/core";
import { pick, setupBrowse } from "./browse";
import { BadgeState, createBadge, element, setBadge } from "./sections";
import { Outcome, errorLine } from "./worker";

/** A number, or the name of a design variable. */
type Coordinate = number | string;

interface Hardpoints {
  points: { name: string; coordinates: Coordinate[] }[];
}

interface Summary {
  steps: number;
  end_time: number;
  bodies: number;
}

/** Pasted paths often carry whitespace or the quotes added by "Copy as path". */
const clean = (path: string) => path.trim().replace(/^"|"$/g, "").trim();

/** `model.json` is solved to `model-solution.json` unless the user chooses another file. */
const defaultOutput = (model: string) => model.replace(/\.json$/i, "") + "-solution.json";

/** One line of the run's progress: a badge, the step and a detail such as the percent done. */
function createStep(label: string) {
  const badge = createBadge("running");
  const detail = element("small");
  const node = element("li");
  node.append(badge, element("span", label), detail);
  return {
    node,
    detail,
    finish: (state: BadgeState) => {
      setBadge(badge, state);
      if (state === "ok") detail.textContent = "";
    },
  };
}

export function setupSolve() {
  const form = document.querySelector<HTMLFormElement>("#solve-form")!;
  const designFields = document.querySelector<HTMLElement>("#solve-design")!;
  const designNote = document.querySelector<HTMLElement>("#solve-design-note")!;
  const points = document.querySelector<HTMLTableSectionElement>("#solve-points")!;
  const status = document.querySelector<HTMLElement>("#solve-status")!;
  const steps = document.querySelector<HTMLElement>("#solve-steps")!;
  const summary = document.querySelector<HTMLElement>("#solve-summary")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const stop = document.querySelector<HTMLButtonElement>("#solve-stop")!;

  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;
  const variableInputs = () => [...points.querySelectorAll<HTMLInputElement>("input[data-variable]")];

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const setBusy = (busy: boolean) => {
    submit.disabled = busy;
  };
  /** Sets a design variable everywhere it is used: the same variable appears in several points. */
  const setVariable = (name: string, value: string) =>
    variableInputs()
      .filter((input) => input.dataset.variable === name)
      .forEach((input) => {
        input.value = value;
        fire(input);
      });

  type Cell = HTMLTableCellElement;
  const textInput = (td: Cell) => td.querySelector<HTMLInputElement>("input[type=text]");
  const rowOf = (td: Cell) => (td.parentElement as HTMLTableRowElement).sectionRowIndex;
  const cellAt = (row: number, column: number) => points.rows[row]?.cells[column] as Cell | undefined;
  const fire = (input: HTMLInputElement) => input.dispatchEvent(new Event("input", { bubbles: true }));
  const isEmpty = (input: HTMLInputElement) => input.value.trim() === "";
  const isBad = (input: HTMLInputElement) => isEmpty(input) || Number.isNaN(Number(input.value));
  const invalidMessage = "Hardpoint values must be numbers with . as the decimal point";

  let selected: Cell[] = [];
  let anchor: Cell | null = null; // where a mouse drag started
  let origin: Cell | null = null; // fixed corner of the selection
  let extent: Cell | null = null; // moving corner of the selection

  const clearSelection = () => {
    selected.forEach((td) => td.classList.remove("selected"));
    selected = [];
  };
  const selectRange = (a: Cell, b: Cell) => {
    clearSelection();
    const [r0, r1] = [rowOf(a), rowOf(b)].sort((x, y) => x - y);
    const [c0, c1] = [a.cellIndex, b.cellIndex].sort((x, y) => x - y);
    for (let r = r0; r <= r1; r++)
      for (let c = c0; c <= c1; c++) {
        const td = cellAt(r, c)!;
        if (textInput(td) && !textInput(td)!.disabled) selected.push(td);
      }
    selected.forEach((td) => td.classList.add("selected"));
    extent = b;
  };

  /** Spreads whitespace-separated rows and columns over the cells from `start`, skipping greyed-out ones. */
  const fill = (start: Cell, text: string) =>
    text
      .trim()
      .split(/\r?\n/)
      .forEach((line, i) =>
        line.trim().split(/\s+/).forEach((value, j) => {
          const target = cellAt(rowOf(start) + i, start.cellIndex + j);
          const input = target && textInput(target);
          if (input && !input.disabled) {
            input.value = value;
            fire(input);
          }
        }),
      );

  /** Moves focus to the next editable cell in a direction. */
  const moveFocus = (from: Cell, [dr, dc]: number[]) => {
    for (let r = rowOf(from) + dr, c = from.cellIndex + dc; points.rows[r]?.cells[c]; r += dr, c += dc) {
      const target = textInput(cellAt(r, c)!);
      if (target && !target.disabled) return target.focus();
    }
  };

  const ARROWS: Record<string, number[]> = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] };

  points.addEventListener("keydown", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.type !== "text") return;
    const td = input.closest("td")!;
    const arrow = ARROWS[event.key];
    if (event.shiftKey && arrow && origin && extent) {
      event.preventDefault();
      const next = cellAt(rowOf(extent) + arrow[0], extent.cellIndex + arrow[1]);
      if (next && textInput(next)) selectRange(origin, next);
    } else if (selected.length > 1 && (event.key === "Delete" || event.key === "Backspace")) {
      event.preventDefault();
      selected.forEach((cell) => {
        textInput(cell)!.value = "";
        fire(textInput(cell)!);
      });
    } else if (arrow || event.key === "Enter") {
      clearSelection();
      // Left and right move cells only at the edge of the text; up, down and Enter always do.
      const sideways = arrow && arrow[1] !== 0;
      const atEdge = event.key === "ArrowLeft" ? input.selectionStart === 0 : input.selectionEnd === input.value.length;
      if (sideways && !atEdge) return;
      event.preventDefault();
      moveFocus(td, arrow ?? [1, 0]);
    }
  });

  points.addEventListener("focusin", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.type !== "text") return;
    origin = extent = input.closest("td");
    requestAnimationFrame(() => input.select());
  });

  points.addEventListener("click", (event) => {
    const td = (event.target as HTMLElement).closest("td");
    if (event.target === td) textInput(td!)?.focus();
  });

  points.addEventListener("dragstart", (event) => event.preventDefault());

  points.addEventListener("mousedown", (event) => {
    const td = (event.target as HTMLElement).closest("td");
    if (event.shiftKey && origin && td && textInput(td)) {
      event.preventDefault();
      selectRange(origin, td);
      return;
    }
    clearSelection();
    anchor = td && textInput(td) ? td : null;
  });

  document.addEventListener("mousemove", (event) => {
    if (!anchor || !(event.buttons & 1)) return;
    const td = document.elementFromPoint(event.clientX, event.clientY)?.closest("td");
    if (!td || !points.contains(td) || !textInput(td) || (td === anchor && !selected.length)) return;
    points.classList.add("selecting");
    textInput(anchor)!.focus();
    selectRange(anchor, td);
  });

  document.addEventListener("mouseup", () => {
    anchor = null;
    points.classList.remove("selecting");
  });

  points.addEventListener("copy", (event) => {
    if (selected.length < 2) return;
    event.preventDefault();
    const lines = new Map<number, string[]>();
    selected.forEach((td) => lines.set(rowOf(td), [...(lines.get(rowOf(td)) ?? []), textInput(td)!.value]));
    event.clipboardData!.setData("text/plain", [...lines.values()].map((line) => line.join("\t")).join("\n"));
  });

  // Some webviews hand over no clipboard data with the paste event, and a text field would drop the line breaks.
  points.addEventListener("paste", async (event) => {
    event.preventDefault();
    const target = event.target as HTMLInputElement;
    const start = selected[0] ?? target.closest("td")!;
    const text = event.clipboardData?.getData("text/plain") || (await navigator.clipboard.readText().catch(() => ""));
    if (selected.length > 1 || /\s/.test(text.trim())) fill(start, text);
    else document.execCommand("insertText", false, text.trim());
  });

  /** A row can only be switched off while it is complete; an incomplete row stays editable with its box greyed out. */
  const rowStates = new WeakMap<HTMLTableRowElement, () => void>();

  points.addEventListener("input", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.type !== "text") return;
    // Whatever route multi-value text took into one cell (paste, drop), spread it over the cells.
    if (/\s/.test(input.value.trim())) {
      const text = input.value;
      input.value = "";
      fill(input.closest("td")!, text);
      return;
    }
    const bad = !isEmpty(input) && isBad(input);
    input.classList.toggle("invalid", bad);
    rowStates.get(input.closest("tr")!)?.();
    if (bad) show(invalidMessage, false);
    else if (!points.querySelector(".invalid")) show("");
  });

  const renderPoints = (hardpoints: Hardpoints) => {
    const kept = new Map(variableInputs().map((input) => [input.dataset.variable!, input.value]));
    points.replaceChildren(
      ...hardpoints.points.map(({ name, coordinates }) => {
        const row = points.insertRow();
        row.dataset.point = name;
        row.insertCell().textContent = name;
        const cells = coordinates.map((coordinate) => {
          const input = document.createElement("input");
          Object.assign(input, { type: "text", inputMode: "decimal", autocomplete: "off" });
          if (typeof coordinate === "string") {
            input.dataset.variable = coordinate;
            input.value = kept.get(coordinate) ?? "";
          } else {
            input.value = input.dataset.default = String(coordinate);
          }
          row.insertCell().append(input);
          return input;
        });
        const box = Object.assign(document.createElement("input"), { type: "checkbox", title: `Adjust ${name}` });
        const sync = () => {
          box.disabled = cells.some(isEmpty);
          const editable = box.checked || box.disabled;
          cells.forEach((input) => {
            input.disabled = !editable;
            if (!editable && input.dataset.default !== undefined) input.value = input.dataset.default;
          });
        };
        box.checked = cells.some((input) => input.dataset.variable);
        box.addEventListener("change", sync);
        rowStates.set(row, sync);
        sync();
        row.insertCell().append(box);
        return row;
      }),
    );
    designFields.hidden = hardpoints.points.length === 0;
    designNote.textContent = "";
  };

  const loadModel = async () => {
    const model = clean(field("model").value);
    if (!model) return;
    try {
      renderPoints(await invoke<Hardpoints>("model_hardpoints", { path: model }));
      if (!clean(field("output").value) || field("output").dataset.generated) {
        field("output").value = defaultOutput(model);
        field("output").dataset.generated = "true";
      }
      show("");
    } catch (error) {
      renderPoints({ points: [] });
      show(String(error), false);
    }
  };

  field("model").addEventListener("change", loadModel);
  // Typing or browsing makes the path the user's own; a later model change keeps it.
  ["input", "change"].forEach((type) =>
    field("output").addEventListener(type, () => delete field("output").dataset.generated),
  );
  setupBrowse(form);

  document.querySelector("#solve-load-design")!.addEventListener("click", async () => {
    const path = await pick(false, ["json"]);
    if (!path) return;
    try {
      const design = await invoke<Record<string, number>>("read_design", { path });
      const names = new Set(variableInputs().map((input) => input.dataset.variable!));
      names.forEach((name) => name in design && setVariable(name, String(design[name])));
      const unused = Object.keys(design).filter((name) => !names.has(name)).length;
      const missing = [...names].filter((name) => !(name in design)).length;
      designNote.textContent = [
        `${names.size - missing} of ${names.size} variables set`,
        unused ? `${unused} in the file not used by this model` : "",
      ]
        .filter(Boolean)
        .join(", ");
    } catch (error) {
      show(String(error), false);
    }
  });

  /** The model, design variable values and adjusted fixed points of the form, as the backend expects them. */
  const readCase = () => ({
    model: clean(field("model").value),
    design: Object.fromEntries(variableInputs().map((input) => [input.dataset.variable!, Number(input.value)])),
    points: Object.fromEntries(
      [...points.rows]
        .filter((row) => row.querySelector("input[data-default]:not(:disabled)"))
        .map((row) => [row.dataset.point!, [...row.querySelectorAll<HTMLInputElement>("input[type=text]")].map((input) => Number(input.value))]),
    ),
  });

  let stopped = false;
  stop.addEventListener("click", () => {
    stopped = true;
    void invoke("cancel_solve");
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const output = clean(field("output").value);
    if (!output) {
      show("Choose a solution file", false);
      return;
    }
    const invalid = [...points.querySelectorAll<HTMLInputElement>("input[type=text]:not(:disabled)")].find(isBad);
    if (invalid) {
      invalid.focus();
      show(isEmpty(invalid) ? "Fill in every hardpoint cell" : invalidMessage, false);
      return;
    }
    const request = {
      ...readCase(),
      output,
      include: [...form.querySelectorAll<HTMLInputElement>("[name=include]:checked")].map((box) => box.value),
      force: field("force").checked,
    };
    stopped = false;
    setBusy(true);
    stop.hidden = false;
    summary.hidden = true;
    show("");
    const solve = createStep("Solve motion");
    const write = createStep("Write solution");
    steps.replaceChildren(solve.node);
    const onProgress = new Channel<number>();
    onProgress.onmessage = (percent) => {
      solve.detail.textContent = `${percent.toFixed(0)} %`;
      if (percent >= 100 && !write.node.isConnected) {
        solve.finish("ok");
        steps.append(write.node);
      }
    };
    try {
      const outcome = await invoke<Outcome>("run_solve", { request, onProgress });
      if (outcome.exit_code === 0) {
        solve.finish("ok");
        if (!write.node.isConnected) steps.append(write.node);
        write.finish("ok");
        await showSummary(output);
      } else {
        (write.node.isConnected ? write : solve).finish("error");
        show(stopped ? "Stopped" : (errorLine(outcome.stderr) ?? `clab exited with ${outcome.exit_code}`), false);
      }
    } catch (error) {
      (write.node.isConnected ? write : solve).finish("error");
      show(String(error), false);
    } finally {
      stop.hidden = true;
      setBusy(false);
    }
  });

  const showSummary = async (output: string) => {
    const { steps, end_time, bodies } = await invoke<Summary>("solution_summary", { path: output });
    show("Solved");
    summary.replaceChildren(
      ...Object.entries({ Solution: output, Steps: steps, "End time": end_time, Bodies: bodies }).flatMap(([name, value]) => [
        element("dt", name),
        element("dd", String(value)),
      ]),
    );
    summary.hidden = false;
  };
}
