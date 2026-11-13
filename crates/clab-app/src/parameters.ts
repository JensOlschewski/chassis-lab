import { fire, setupGrid } from "./grid";
import { element } from "./sections";

export type DesignValue = number | boolean | string;

type Kind = "number" | "switch" | "text" | "expression";

/** One editable value of the model: a literal `value`, or the name of a design `variable`. */
interface Cell {
  pointer: string;
  column: number;
  span: number;
  kind: Kind;
  label: string;
  variable: string | null;
  value: unknown;
  locked: boolean;
}

/** A table of related model values, as the backend lists them. */
export interface Section {
  id: string;
  title: string;
  columns: string[];
  rows: { name: string; cells: Cell[] }[];
}

export const decimalMessage = "Numbers need . as the decimal point";
export const invalidMessage = (input: HTMLInputElement) => (isEmpty(input) ? "Fill in every cell" : decimalMessage);

const isEmpty = (input: HTMLInputElement) => input.type === "text" && input.value.trim() === "";
const isBad = (input: HTMLInputElement) =>
  isEmpty(input) || (input.dataset.kind === "number" && Number.isNaN(Number(input.value)));

const inputs = (root: ParentNode) => [...root.querySelectorAll<HTMLInputElement>("input[data-kind]")];

/** An input's content as text; a switch reads "true" or "false". */
const read = (input: HTMLInputElement) => (input.type === "checkbox" ? String(input.checked) : input.value);

/** Sets an input without announcing it. A switch is on for "true" and "1". */
const write = (input: HTMLInputElement, text: string) => {
  if (input.type === "checkbox") input.checked = text === "true" || text === "1";
  else input.value = text;
};

/** The model's own value of a cell, as the user would type it. */
const literal = (cell: Cell) => String(cell.value);

function createInput(cell: Cell, kept: Map<string, string>) {
  const input = document.createElement("input");
  Object.assign(input.dataset, { kind: cell.kind, pointer: cell.pointer });
  input.title = cell.label;
  if (cell.kind === "switch") {
    input.type = "checkbox";
  } else {
    Object.assign(input, { type: "text", autocomplete: "off", inputMode: cell.kind === "number" ? "decimal" : "text" });
  }
  if (cell.variable) {
    input.dataset.variable = cell.variable;
    write(input, kept.get(cell.variable) ?? (cell.kind === "switch" ? "false" : ""));
  } else {
    input.dataset.default = literal(cell);
    write(input, literal(cell));
    input.disabled = cell.locked;
  }
  return input;
}

/** A foldable table of one section. Variable inputs take their text from `kept`, by variable name. */
export function createSection(section: Section, kept: Map<string, string>, open: boolean) {
  const body = document.createElement("tbody");

  section.rows.forEach(({ name, cells }) => {
    const row = body.insertRow();
    row.insertCell().textContent = name;
    let next = 0;
    cells.forEach((cell) => {
      for (; next < cell.column; next++) row.insertCell();
      const td = row.insertCell();
      td.colSpan = cell.span;
      td.append(createInput(cell, kept));
      next = cell.column + cell.span;
    });
    for (; next < section.columns.length; next++) row.insertCell();
  });

  setupGrid(body, (input) => {
    input.classList.toggle("invalid", !isEmpty(input) && isBad(input));
  });

  const head = element("tr");
  ["", ...section.columns].forEach((title) => head.append(element("th", title)));
  const table = element("table", "", "grid");
  table.createTHead().append(head);
  table.append(body);

  const details = element("details");
  details.open = open;
  details.append(element("summary", section.title), element("div", "", "grid-scroll"));
  details.lastElementChild!.append(table);
  return details;
}

/** The text of every design variable input, by variable name. */
export const keptVariables = (root: ParentNode) =>
  new Map(inputs(root).flatMap((input) => (input.dataset.variable ? [[input.dataset.variable, read(input)] as const] : [])));

/** Sets a design variable in every cell that uses it: the same variable appears in several places. */
export function setVariable(root: ParentNode, name: string, value: DesignValue) {
  inputs(root)
    .filter((input) => input.dataset.variable === name)
    .forEach((input) => {
      write(input, String(value));
      fire(input);
    });
}

export const variableNames = (root: ParentNode) => new Set(keptVariables(root).keys());

/** The design variable values, and the literals the user changed (by JSON pointer), as the backend expects them. */
export function readParameters(root: ParentNode) {
  const design: Record<string, DesignValue> = {};
  const overrides: Record<string, number | string> = {};
  for (const input of inputs(root)) {
    const { kind, variable, pointer } = input.dataset;
    if (variable) {
      design[variable] = kind === "number" ? Number(input.value) : kind === "switch" ? input.checked : input.value;
    } else if (read(input) !== input.dataset.default) {
      overrides[pointer!] = kind === "number" ? Number(input.value) : kind === "switch" ? Number(input.checked) : input.value;
    }
  }
  return { design, overrides };
}

/** The first cell the user must still fix, with its section opened so it can be seen. */
export function firstInvalid(root: ParentNode) {
  const input = inputs(root).find((input) => input.type === "text" && !input.disabled && isBad(input));
  input?.closest("details")!.setAttribute("open", "");
  return input;
}
