import { element } from "./sections";

const WORD = /[A-Za-z0-9_.]/;
const SHOWN = 50;

/** Positions of `query` in `text` as a case-insensitive subsequence, with a score; `null` when it does not match. */
export function fuzzy(query: string, text: string): { score: number; at: number[] } | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  const at: number[] = [];
  let score = 0;
  let from = 0;
  for (const char of q) {
    const index = t.indexOf(char, from);
    if (index < 0) return null;
    // Runs of letters and starts of words (after `.` or `_`) count most.
    if (index === from && at.length) score += 3;
    if (index === 0 || ".".includes(t[index - 1]) || t[index - 1] === "_") score += 2;
    at.push(index);
    from = index + 1;
  }
  if (t.includes(q)) score += 5;
  return { score: score - text.length / 100, at };
}

/** The identifier-like text left of the caret, and where it starts. */
function tokenAt(input: HTMLInputElement) {
  const caret = input.selectionStart ?? input.value.length;
  let start = caret;
  while (start > 0 && WORD.test(input.value[start - 1])) start--;
  return { start, caret, text: input.value.slice(start, caret) };
}

let popup: HTMLElement | null = null;

const close = () => {
  popup?.remove();
  popup = null;
};

/**
 * Suggestions while typing in `input`, like an editor's: the word at the caret is matched fuzzily against
 * `candidates()`, the matching letters are bold. Arrows choose, Tab or Enter accepts, Esc closes.
 * A candidate ending in `(` is a function and keeps the bracket.
 */
export function attachCompletion(input: HTMLInputElement, candidates: () => string[]) {
  let matches: { text: string; at: number[] }[] = [];
  let selected = 0;

  const render = () => {
    if (!matches.length) return close();
    if (!popup) {
      popup = element("ul", "", "completion");
      // Clicking a suggestion must not take the focus from the cell.
      popup.addEventListener("mousedown", (event) => event.preventDefault());
      document.body.append(popup);
    }
    const box = input.getBoundingClientRect();
    Object.assign(popup.style, { left: `${box.left}px`, top: `${box.bottom + 2}px`, minWidth: `${Math.min(box.width, 480)}px` });
    popup.replaceChildren(
      ...matches.map((match, index) => {
        const item = element("li", "", index === selected ? "selected" : "");
        const marked = new Set(match.at);
        [...match.text].forEach((char, i) => item.append(marked.has(i) ? element("b", char) : document.createTextNode(char)));
        item.addEventListener("click", () => accept(index));
        return item;
      }),
    );
    popup.querySelector(".selected")?.scrollIntoView({ block: "nearest" });
  };

  const update = () => {
    const { text } = tokenAt(input);
    selected = 0;
    if (!text) {
      matches = [];
      return render();
    }
    matches = candidates()
      .flatMap((candidate) => {
        const found = fuzzy(text, candidate);
        return found && candidate !== text ? [{ text: candidate, ...found }] : [];
      })
      .sort((a, b) => b.score - a.score)
      .slice(0, SHOWN);
    render();
  };

  const accept = (index: number) => {
    const { start, caret } = tokenAt(input);
    input.setRangeText(matches[index].text, start, caret, "end");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    matches = [];
    close();
  };

  input.addEventListener("input", update);
  input.addEventListener("blur", () => {
    matches = [];
    close();
  });
  // Runs before the grid's own key handling on the table body, so the arrows and Enter belong to the popup while it is open.
  input.addEventListener("keydown", (event) => {
    if (!popup || !matches.length) return;
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step) {
      selected = (selected + step + matches.length) % matches.length;
      render();
    } else if (event.key === "Tab" || event.key === "Enter") {
      accept(selected);
    } else if (event.key === "Escape") {
      matches = [];
      close();
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  });
}
