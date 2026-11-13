type Cell = HTMLTableCellElement;

const ARROWS: Record<string, number[]> = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] };

/** Texts and expressions keep their spaces; only numbers are split over several cells. */
const isFree = (input: HTMLInputElement) => input.dataset.kind === "text" || input.dataset.kind === "expression";

export const fire = (input: HTMLInputElement) => input.dispatchEvent(new Event("input", { bubbles: true }));

/** The drag in progress, if any. The mouse may leave the table while it lasts, so one listener pair serves all grids. */
let drag: { move: (event: MouseEvent) => void; end: () => void } | null = null;
document.addEventListener("mousemove", (event) => drag?.move(event));
document.addEventListener("mouseup", () => {
  drag?.end();
  drag = null;
});

/**
 * Spreadsheet behaviour for a table body of text inputs: arrow-key and Enter navigation, mouse and Shift
 * selection of cell ranges, Delete, copy, and paste of several rows and columns.
 * `onInput` runs after each edit of a cell.
 */
export function setupGrid(body: HTMLTableSectionElement, onInput: (input: HTMLInputElement) => void) {
  const textInput = (td: Cell | null | undefined) => td?.querySelector<HTMLInputElement>("input[type=text]") ?? null;
  const rowOf = (td: Cell) => (td.parentElement as HTMLTableRowElement).sectionRowIndex;
  const cellAt = (row: number, column: number) => body.rows[row]?.cells[column] as Cell | undefined;
  const editable = (td: Cell | null | undefined) => {
    const input = textInput(td);
    return input && !input.disabled ? input : null;
  };

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
        const td = cellAt(r, c);
        if (td && editable(td)) selected.push(td);
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
          const input = editable(cellAt(rowOf(start) + i, start.cellIndex + j));
          if (input) {
            input.value = value;
            fire(input);
          }
        }),
      );

  /** Moves focus to the next editable cell in a direction. */
  const moveFocus = (from: Cell, [dr, dc]: number[]) => {
    for (let r = rowOf(from) + dr, c = from.cellIndex + dc; cellAt(r, c); r += dr, c += dc) {
      const target = editable(cellAt(r, c));
      if (target) return target.focus();
    }
  };

  body.addEventListener("keydown", (event) => {
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

  body.addEventListener("focusin", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.type !== "text") return;
    origin = extent = input.closest("td");
    requestAnimationFrame(() => input.select());
  });

  body.addEventListener("click", (event) => {
    const td = (event.target as HTMLElement).closest("td");
    if (event.target === td) textInput(td)?.focus();
  });

  body.addEventListener("dragstart", (event) => event.preventDefault());

  body.addEventListener("mousedown", (event) => {
    const td = (event.target as HTMLElement).closest("td");
    if (event.shiftKey && origin && td && textInput(td)) {
      event.preventDefault();
      selectRange(origin, td);
      return;
    }
    clearSelection();
    anchor = td && textInput(td) ? td : null;
    drag = {
      move: (move) => {
        if (!anchor || !(move.buttons & 1)) return;
        const over = document.elementFromPoint(move.clientX, move.clientY)?.closest("td");
        if (!over || !body.contains(over) || !textInput(over) || (over === anchor && !selected.length)) return;
        body.classList.add("selecting");
        textInput(anchor)!.focus();
        selectRange(anchor, over);
      },
      end: () => {
        anchor = null;
        body.classList.remove("selecting");
      },
    };
  });

  body.addEventListener("copy", (event) => {
    if (selected.length < 2) return;
    event.preventDefault();
    const lines = new Map<number, string[]>();
    selected.forEach((td) => lines.set(rowOf(td), [...(lines.get(rowOf(td)) ?? []), textInput(td)!.value]));
    event.clipboardData!.setData("text/plain", [...lines.values()].map((line) => line.join("\t")).join("\n"));
  });

  // Some webviews hand over no clipboard data with the paste event, and a text field would drop the line breaks.
  body.addEventListener("paste", async (event) => {
    const target = event.target as HTMLInputElement;
    if (target.type !== "text" || (isFree(target) && selected.length < 2)) return;
    event.preventDefault();
    const start = selected[0] ?? target.closest("td")!;
    const text = event.clipboardData?.getData("text/plain") || (await navigator.clipboard.readText().catch(() => ""));
    if (selected.length > 1 || /\s/.test(text.trim())) fill(start, text);
    else document.execCommand("insertText", false, text.trim());
  });

  body.addEventListener("input", (event) => {
    const input = event.target as HTMLInputElement;
    if (input.type !== "text") return;
    // Whatever route multi-value text took into one number cell (paste, drop), spread it over the cells.
    if (!isFree(input) && /\s/.test(input.value.trim())) {
      const text = input.value;
      input.value = "";
      fill(input.closest("td")!, text);
      return;
    }
    onInput(input);
  });
}
