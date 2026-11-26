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

/** A drive signal that is not defined is `~` in the model and an empty cell here. */
const UNDEFINED = "~";

const isEmpty = (input: HTMLInputElement) =>
  input.type === "text" && input.dataset.kind !== "expression" && input.value.trim() === "";
/** A design variable shows as its quoted name, `"HP_L_X"`, in a number or text cell; a switch shows its value. */
const NAMED = /^"([A-Za-z][A-Za-z0-9_]*)"$/;
const nameIn = (input: HTMLInputElement) => NAMED.exec(input.value.trim())?.[1];
const holdsName = (input: HTMLInputElement) => input.dataset.kind === "number" || input.dataset.kind === "text";
export const isNamed = (input: HTMLInputElement) => holdsName(input) && nameIn(input) !== undefined;
/** Numbers must be numbers, texts (characteristic files) `.json` paths, unless the cell holds a name. */
const isBad = (input: HTMLInputElement) =>
  isEmpty(input) ||
  (!isNamed(input) &&
    ((input.dataset.kind === "number" && Number.isNaN(Number(input.value))) ||
      (input.dataset.kind === "text" && !/\.json$/i.test(input.value.trim()))));

const inputs = (root: ParentNode) => [...root.querySelectorAll<HTMLInputElement>("input[data-kind]")];

/** An input's content as text; a switch reads "true" or "false". */
const read = (input: HTMLInputElement) => (input.type === "checkbox" ? String(input.checked) : input.value);

/** Sets an input without announcing it. A switch is on for "true" and "1". */
const write = (input: HTMLInputElement, text: string) => {
  if (input.type === "checkbox") input.checked = text === "true" || text === "1";
  else input.value = text;
};

/** The model's own value of a cell, as the user would type it. */
const literal = (cell: Cell) => (cell.kind === "expression" && cell.value === UNDEFINED ? "" : String(cell.value));

/** The model's own value of an input, which `modelValue` compares against. */
const modelDefault = (input: HTMLInputElement) => {
  const original = input.dataset.default!;
  const { kind } = input.dataset;
  if (kind === "number") return Number(original);
  if (kind === "switch") return Number(original === "true" || original === "1");
  return kind === "expression" ? original || UNDEFINED : original;
};

/** An input's value as the model holds it. */
const modelValue = (input: HTMLInputElement) => {
  const { kind, off } = input.dataset;
  if (off === "true") return UNDEFINED;
  if (kind === "number") return Number(input.value);
  if (kind === "switch") return Number(input.checked);
  return kind === "expression" ? input.value.trim() || UNDEFINED : input.value;
};

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
    if (cell.kind === "switch") write(input, kept.get(cell.variable) ?? "false");
    else write(input, `"${cell.variable}"`);
  } else {
    input.dataset.default = literal(cell);
    write(input, literal(cell));
    input.disabled = cell.locked;
  }
  input.classList.toggle("named", isNamed(input));
  return input;
}

/** Drive signals have no `active` value in the model: a drive is active when any of its signals is defined. */
function createDriveToggle(row: HTMLTableRowElement, cells: Cell[]) {
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.title = "active";
  toggle.checked = cells.some((cell) => cell.value !== UNDEFINED);
  const apply = () =>
    inputs(row).forEach((input) => {
      input.disabled = !toggle.checked;
      input.dataset.off = String(!toggle.checked);
      fire(input);
    });
  toggle.addEventListener("change", apply);
  row.insertCell().append(toggle);
  return apply;
}

/** A foldable table of one section. Variable inputs take their text from `kept`, by variable name. */
export function createSection(section: Section, kept: Map<string, string>, open: boolean) {
  const body = document.createElement("tbody");
  const drives = section.id === "drives";

  section.rows.forEach(({ name, cells }) => {
    const row = body.insertRow();
    row.insertCell().textContent = name;
    const applyToggle = drives ? createDriveToggle(row, cells) : undefined;
    let next = 0;
    cells.forEach((cell) => {
      for (; next < cell.column; next++) row.insertCell();
      const td = row.insertCell();
      td.colSpan = cell.span;
      td.append(createInput(cell, kept));
      next = cell.column + cell.span;
    });
    for (; next < section.columns.length; next++) row.insertCell();
    applyToggle?.();
  });

  setupGrid(body, (input) => {
    input.classList.toggle("invalid", !isEmpty(input) && isBad(input));
    input.classList.toggle("named", isNamed(input));
  });

  const head = element("tr");
  ["", ...(drives ? ["Active"] : []), ...section.columns].forEach((title) => head.append(element("th", title)));
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

/** Sets a design variable switch in every cell that uses it; number and text cells keep showing the name. */
export function setVariable(root: ParentNode, name: string, value: DesignValue) {
  inputs(root)
    .filter((input) => input.dataset.variable === name && input.type === "checkbox")
    .forEach((input) => {
      write(input, String(value));
      fire(input);
    });
}

export const variableNames = (root: ParentNode) => new Set(keptVariables(root).keys());

/**
 * The design variable values, and the literals the user changed (by JSON pointer), as the backend expects them.
 * Number and text variables take their values from a design file, so only switches give one here. A cell with a
 * new name goes in as that name; a plain cell named so gets its model value. A variable cell given a value stops
 * being a variable.
 */
export function readParameters(root: ParentNode) {
  const design: Record<string, DesignValue> = {};
  const overrides: Record<string, number | string> = {};
  const named: Record<string, DesignValue> = {};
  for (const input of inputs(root)) {
    const { kind, variable, pointer } = input.dataset;
    const name = isNamed(input) ? nameIn(input)! : undefined;
    if (kind === "switch" && variable) {
      design[variable] = input.checked;
    } else if (name !== undefined) {
      if (name === variable) continue;
      overrides[pointer!] = name;
      if (!variable) named[name] ??= kind === "number" ? Number(input.dataset.default) : input.dataset.default!;
    } else if (variable) {
      overrides[pointer!] = kind === "number" ? Number(input.value) : input.value.trim();
    } else if (modelValue(input) !== modelDefault(input)) {
      overrides[pointer!] = modelValue(input);
    }
  }
  return { design: { ...named, ...design }, overrides };
}

/** The first cell the user must still fix, with its section opened so it can be seen. */
export function firstInvalid(root: ParentNode) {
  const input = inputs(root).find((input) => input.type === "text" && !input.disabled && isBad(input));
  input?.closest("details")!.setAttribute("open", "");
  return input;
}
