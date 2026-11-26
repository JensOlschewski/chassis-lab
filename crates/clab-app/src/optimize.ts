import { Channel, invoke } from "@tauri-apps/api/core";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { pick, setupBrowse } from "./browse";
import { attachCompletion } from "./complete";
import { setupGrid } from "./grid";
import { Help, helpMark, setupHelp } from "./help";
import { Section } from "./parameters";
import { element } from "./sections";
import { Outcome, errorLine } from "./worker";

/** `problem.toml` as the backend reads and writes it; model paths are absolute. */
interface Problem {
  models: Record<string, string>;
  optimizer: { max_iterations?: number; max_evaluations?: number };
  design: Record<string, [number, number]>;
  signals: Record<string, string>;
  objectives: Record<string, { value: string; weight?: number; scale?: number }>;
  constraints: Record<string, string>;
}

interface ModelNames {
  bodies: Record<string, string[]>;
  joints: string[];
}


const clean = (path: string) => path.trim().replace(/^"|"$/g, "").trim();

/** Body, point and joint paths as `docs/optimize-problem.md` lists them. */
function paths(alias: string, names: ModelNames): string[] {
  const m = alias;
  const list = [`${m}.time`, `${m}.characteristics.`];
  for (const [body, points] of Object.entries(names.bodies)) {
    const b = `${m}.bodies.${body}`;
    list.push(`${b}.velocity.linear`, `${b}.velocity.angular`, `${b}.cm.position`);
    for (const point of points) {
      list.push(`${b}.points.${point}.position`, `${b}.points.${point}.velocity.linear`);
    }
  }
  for (const joint of names.joints) {
    const j = `${m}.joints.${joint}`;
    for (const side of ["i", "j"]) list.push(`${j}.${side}.position`, `${j}.${side}.reaction.force`, `${j}.${side}.reaction.moment`);
  }
  return list;
}

/** Functions an expression may call, as `docs/optimize-problem.md` lists them; a bracket marks a call. */
const FUNCTIONS = "abs sqrt atan atan2 rad2deg deg2rad round numel max min mean gradient cross dot vecnorm interp1".split(" ").map((name) => `${name}(`);

type Kind = "models" | "optimizer" | "design" | "signals" | "objectives" | "constraints";

/** One column of a section table; `expression` cells keep their spaces (see `grid.ts`). */
interface Column {
  name: string;
  title: string;
  kind: "number" | "text" | "expression";
  /** Cells of this column suggest design variables of the models, or (expressions) results, signals and functions. */
  complete?: "variables" | "formula";
}

/** The foldable tables of the problem, in file order. The optimizer table has one fixed row. */
const SECTIONS: { kind: Kind; title: string; subtitle?: string; columns: Column[]; help?: Help }[] = [
  {
    kind: "models",
    title: "Model files",
    subtitle: "The model files which are used during the optimization",
    help: [
      ["Alias", "The short name optimization expressions use to reach the model, e.g. pwt.characteristics.wheel_velocities.l_wc"],
      ["Details", "Model details opens the model in Solve, where its design variables can be inspected and modified. After change model needs to be saved and loaded again in Model files."],
    ],
    columns: [
      { name: "name", title: "Alias", kind: "text" },
      { name: "path", title: "Path", kind: "expression" },
    ],
  },
  {
    kind: "design",
    title: "Design Variables",
    subtitle: "The model parameters the optimizer is able to change",
    help: [
      ["Variable", "The name exactly as the model file uses it. A model only receives the variables its file mentions."],
      ["Start", "Start value is taken from optional defined design file above. Without the midpoint of the bounds is used."],
      ["Fixed", "A variable in the start design file that is not listed here stays unchangeable by the optimizer."],
      ["Create", "In Solve, write a quoted name such as \"HP_L_Z\" in a cell, then Save model. Load that saved model in Model files."],
    ],
    columns: [
      { name: "name", title: "Variable", kind: "text", complete: "variables" },
      { name: "lower", title: "Lower", kind: "number" },
      { name: "upper", title: "Upper", kind: "number" },
    ],
  },
  {
    kind: "signals",
    title: "Signals",
    subtitle: "A named value computed from model results or other signals. Objectives and constraints use it by name.",
    help: [
      ["Order", "Signals can use each other in any order"],
      ["Names", "Letters, digits and _, starting with a letter. Reserved don't use! x, y, z, xyz, end, s, pi or a function name."],
      ["Functions", "The following Matlab functions can be used: abs sqrt atan atan2 rad2deg deg2rad round numel max min mean gradient cross dot vecnorm interp1, use MATLAB help for details"],
      ["Operators", "+ - * / ^ .* ./ .^ and pi. Use .x .y .z to pick a component of a vector."],
      ["Index", "One index along time: val(mid), val(2:end), val(round(end/2))."],
    ],
    columns: [
      { name: "name", title: "Name", kind: "text" },
      { name: "value", title: "Expression", kind: "expression", complete: "formula" },
    ],
  },
  {
    kind: "objectives",
    title: "Objectives (minimized)",
    subtitle: "What the optimizer minimizes",
    help: [
      ["Goal", "Minimize $J = \\tfrac{\\sum_i w_i f_i / s_i}{\\sum_i w_i}$"],
      ["Value", "$f_i$ is the expression defined to minimize. To maximize use -"],
      ["Weight", "$w_i$ is relative importance, default 1."],
      ["Scale", "$s_i$ normalize objectives of different size. Default: $|f_i|$ at the start design. Define if $|f_i|$ $\\approx 0$"],
    ],
    columns: [
      { name: "name", title: "Name", kind: "text" },
      { name: "value", title: "Expression", kind: "expression", complete: "formula" },
      { name: "weight", title: "Weight (1)", kind: "number" },
      { name: "scale", title: "Scale (|start|)", kind: "number" },
    ],
  },
  {
    kind: "constraints",
    title: "Constraints",
    subtitle: "Limits the design must keep",
    help: [
      ["What", "A limit the design must keep. Both sides can be nonlinear"],
      ["Example", "$x \\le 10$ keeps $x$ below or equal to $10$"],
      ["Check", "The result marks a limit VIOLATED when it is not met."],
    ],
    columns: [
      { name: "name", title: "Name", kind: "text" },
      { name: "value", title: "expression", kind: "expression", complete: "formula" },
    ],
  },
  {
    kind: "optimizer",
    title: "Optimizer Options",
    subtitle: "Iteration and evaluation limits for fmincon",
    help: [
      ["Iterations", "Maximum fmincon steps. Empty uses 400"],
      ["Evaluations", "Maximum model evaluations. Empty uses 100 × the number of design variables."],
    ],
    columns: [
      { name: "max_iterations", title: "Max iterations", kind: "number" },
      { name: "max_evaluations", title: "Max evaluations", kind: "number" },
    ],
  },
];

/** `model.toml` gives `model-optimized.json` unless the user chooses another file. */
const defaultOutput = (problem: string) => problem.replace(/\.toml$/i, "") + "-optimized.json";

export function setupOptimize(openInSolve: (path: string) => void) {
  const form = document.querySelector<HTMLFormElement>("#optimize-form")!;
  const status = document.querySelector<HTMLElement>("#optimize-status")!;
  const log = document.querySelector<HTMLElement>("#optimize-log")!;
  const adjust = document.querySelector<HTMLInputElement>("#optimize-adjust")!;
  const adjusted = document.querySelector<HTMLElement>("#optimize-adjusted")!;
  const sectionsRoot = document.querySelector<HTMLElement>("#optimize-sections")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const stop = document.querySelector<HTMLButtonElement>("#optimize-stop")!;
  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;

  /** Suggestions: model results and design variables follow the models. */
  let modelPaths: string[] = [];
  let modelVariables: string[] = [];

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  /** Models and Optimizer are plain field rows; the other sections are grid tables (`tbody`). */
  const bodies = new Map<Kind, HTMLElement>();

  const addRow = (kind: Kind, values: string[] = []) => {
    const { columns } = SECTIONS.find((s) => s.kind === kind)!;
    if (kind === "models") return addModelRow(values);
    if (kind === "optimizer") return addOptimizerRow(values);
    const row = (bodies.get(kind) as HTMLTableSectionElement).insertRow();
    columns.forEach((column, index) => {
      const input = document.createElement("input");
      Object.assign(input, { type: "text", name: column.name, value: values[index] ?? "", autocomplete: "off", title: column.title });
      input.dataset.kind = column.kind;
      if (column.kind === "number") input.inputMode = "decimal";
      if (column.complete === "variables") attachCompletion(input, () => modelVariables);
      if (column.complete === "formula") attachCompletion(input, formulaNames);
      row.insertCell().append(input);
    });
    const remove = element("button", "×");
    Object.assign(remove, { type: "button", ariaLabel: "Remove row", title: "Remove row" });
    remove.addEventListener("click", () => row.remove());
    row.insertCell().append(remove);
  };

  const textField = (name: string, value: string, placeholder: string, className = "") => {
    const input = document.createElement("input");
    Object.assign(input, { type: "text", name, value, placeholder, autocomplete: "off", ariaLabel: placeholder, className });
    return input;
  };

  /** Alias, path, browse, remove and details on one line, as Analyze lists its inputs. */
  const addModelRow = ([alias = "", path = ""]: string[]) => {
    const row = element("div", "", "input-row");
    const browse = element("button", "Browse…");
    Object.assign(browse, { type: "button" });
    const pathInput = textField("path", path, "model .json", "path");
    browse.addEventListener("click", async () => {
      const picked = await pick(false, ["json"]);
      if (!picked) return;
      pathInput.value = picked;
      void refreshNames();
    });
    const remove = element("button", "×");
    Object.assign(remove, { type: "button", ariaLabel: "Remove model", title: "Remove model" });
    remove.addEventListener("click", () => {
      row.remove();
      void refreshNames();
    });
    const details = element("button", "Model details");
    Object.assign(details, { type: "button", title: "Open the model in Solve with its parameters" });
    details.addEventListener("click", () => {
      if (clean(pathInput.value)) openInSolve(clean(pathInput.value));
      else show("Choose a model file first", false);
    });
    row.append(textField("name", alias, "alias", "name"), pathInput, browse, remove, details);
    bodies.get("models")!.append(row);
  };

  /** The two limits as labelled fields; both are optional. */
  const addOptimizerRow = ([iterations = "", evaluations = ""]: string[]) => {
    const row = element("div", "", "option-row");
    for (const [name, title, value] of [["max_iterations", "Max iterations", iterations], ["max_evaluations", "Max evaluations", evaluations]]) {
      const label = element("label", title);
      const input = textField(name, value, "default");
      input.inputMode = "decimal";
      input.addEventListener("input", () => input.classList.toggle("invalid", input.value.trim() !== "" && Number.isNaN(Number(input.value))));
      label.append(input);
      row.append(label);
    }
    bodies.get("optimizer")!.append(row);
  };

  // One help dialog for the pane: Esc, Close and a click on the backdrop close it.
  const showHelp = setupHelp(document.querySelector<HTMLDialogElement>("#optimize-help")!);

  /** Builds the empty foldable tables once; `fill` adds the rows. */
  SECTIONS.forEach(({ kind, title, subtitle, columns, help }) => {
    const plain = kind === "models" || kind === "optimizer";
    const scroll = element("div", "", "grid-scroll");
    let body: HTMLElement;
    if (plain) {
      body = element("div", "", "inputs");
    } else {
      const table = element("table", "", "grid");
      const head = table.createTHead().insertRow();
      [...columns.map((c) => c.title), ""].forEach((text) => head.append(element("th", text)));
      body = table.createTBody();
      setupGrid(body as HTMLTableSectionElement, (input) => {
        const bad = input.dataset.kind === "number" && input.value.trim() !== "" && Number.isNaN(Number(input.value));
        input.classList.toggle("invalid", bad);
      });
      scroll.append(table);
    }
    bodies.set(kind, body);
    const content = plain ? body : scroll;
    if (kind === "models") body.addEventListener("change", () => void refreshNames());
    const details = element("details");
    details.open = false;
    details.dataset.kind = kind;
    const summary = element("summary", title);
    if (help) summary.append(helpMark(title, () => showHelp(title, subtitle, help)));
    details.append(summary, content);
    if (kind !== "optimizer") {
      const add = element("button", "+ Add row");
      add.type = "button";
      add.dataset.add = kind;
      add.addEventListener("click", () => addRow(kind));
      details.append(add);
    }
    sectionsRoot.append(details);
  });

  const rows = (kind: Kind) =>
    [...bodies.get(kind)!.children].map((row) =>
      Object.fromEntries([...row.querySelectorAll("input")].map((input) => [input.name, input.value.trim()])),
    );

  const formulaNames = () => [...modelPaths, ...rows("signals").map((row) => row.name).filter(Boolean), ...FUNCTIONS];

  /** Search paths and design variable names follow the models. */
  const refreshNames = async () => {
    const found: string[] = [];
    const variables = new Set<string>();
    for (const { name, path } of rows("models")) {
      if (!path) continue;
      try {
        found.push(...paths(name || "m", await invoke<ModelNames>("model_names", { path: clean(path) })));
        const sections = await invoke<Section[]>("model_parameters", { path: clean(path) });
        sections.forEach((s) => s.rows.forEach((r) => r.cells.forEach((c) => c.variable && variables.add(c.variable))));
      } catch (error) {
        show(String(error), false);
      }
    }
    modelPaths = found;
    modelVariables = [...variables].sort();
  };

  const number = (text: string | undefined, what: string) => {
    if (!text) return undefined;
    const value = Number(text);
    if (Number.isNaN(value)) throw new Error(`${what}: ${text} is not a number (use . as the decimal point)`);
    return value;
  };

  /** The tables as a problem; throws on a cell that is not a number or a row without a name. */
  const readProblem = (): Problem => {
    const named = (kind: Kind) =>
      rows(kind)
        .filter((row) => Object.values(row).some(Boolean))
        .map((row) => {
          if (!row.name) throw new Error(`A row of ${kind} needs a name`);
          return row;
        });
    const entries = <T>(kind: Kind, value: (row: Record<string, string>) => T) =>
      Object.fromEntries(named(kind).map((row) => [row.name, value(row)]));
    const optimizer = rows("optimizer")[0] ?? {};
    return {
      models: entries("models", (row) => clean(row.path)),
      optimizer: {
        max_iterations: number(optimizer.max_iterations, "Max iterations"),
        max_evaluations: number(optimizer.max_evaluations, "Max evaluations"),
      },
      design: entries("design", (row) => {
        const lower = number(row.lower, row.name);
        const upper = number(row.upper, row.name);
        if (lower === undefined || upper === undefined) throw new Error(`${row.name} needs a lower and an upper bound`);
        return [lower, upper];
      }),
      signals: entries("signals", (row) => row.value),
      objectives: entries("objectives", (row) => ({
        value: row.value,
        weight: number(row.weight, row.name),
        scale: number(row.scale, row.name),
      })),
      constraints: entries("constraints", (row) => row.value),
    };
  };

  const fill = (problem: Problem) => {
    bodies.forEach((body) => body.replaceChildren());
    Object.entries(problem.models).forEach(([name, path]) => addRow("models", [name, path]));
    addRow("optimizer", [
      problem.optimizer?.max_iterations?.toString() ?? "",
      problem.optimizer?.max_evaluations?.toString() ?? "",
    ]);
    Object.entries(problem.design).forEach(([name, [lower, upper]]) => addRow("design", [name, String(lower), String(upper)]));
    Object.entries(problem.signals ?? {}).forEach(([name, value]) => addRow("signals", [name, value]));
    Object.entries(problem.objectives ?? {}).forEach(([name, o]) =>
      addRow("objectives", [name, o.value, o.weight?.toString() ?? "", o.scale?.toString() ?? ""]),
    );
    Object.entries(problem.constraints ?? {}).forEach(([name, value]) => addRow("constraints", [name, value]));
    void refreshNames();
  };

  /** The problem file as read; the tables start from it again on each load. */
  let loaded: Problem | null = null;

  const showAdjusted = () => {
    adjusted.hidden = !adjust.checked || !loaded;
    document.querySelector<HTMLElement>("#optimize-save")!.hidden = adjusted.hidden;
  };
  adjust.addEventListener("change", showAdjusted);

  const loadProblem = async () => {
    const path = clean(field("problem").value);
    loaded = null;
    if (!path) return showAdjusted();
    try {
      loaded = await invoke<Problem>("load_problem", { path });
      fill(loaded);
      show("");
    } catch (error) {
      show(String(error), false);
    }
    showAdjusted();
  };

  field("problem").addEventListener("change", loadProblem);
  setupBrowse(form);

  document.querySelector("#optimize-save")!.addEventListener("click", async () => {
    try {
      const problem = readProblem();
      const source = clean(field("problem").value);
      // The save dialog already asks before replacing a file.
      const path = await save({
        filters: [{ name: "toml", extensions: ["toml"] }],
        defaultPath: source.replace(/\.toml$/i, "") + "-adjusted.toml",
      });
      if (!path) return;
      await invoke("save_problem", { path, problem, force: true });
      show(`Saved ${path}`);
    } catch (error) {
      show(error instanceof Error ? error.message : String(error), false);
    }
  });

  let stopped = false;
  stop.addEventListener("click", () => {
    stopped = true;
    void invoke("cancel_solve");
  });

  const confirmMidpoint = (problem: Problem) => {
    const values = Object.entries(problem.design).map(([name, [lower, upper]]) => `${name} = ${(lower + upper) / 2}`);
    return ask(`No start design given. Start from the midpoint of the bounds?\n\n${values.join("\n")}`, {
      title: "Start design",
      okLabel: "Start",
      cancelLabel: "Cancel",
    });
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const path = clean(field("problem").value);
    if (!loaded) {
      show(path ? "The problem file could not be read" : "Choose a problem file", false);
      return;
    }
    let problem: Problem;
    try {
      problem = adjust.checked ? readProblem() : loaded;
    } catch (error) {
      show(error instanceof Error ? error.message : String(error), false);
      return;
    }
    const dryRun = field("dry_run").checked;
    // The result goes beside the problem file; a dry run writes none.
    const output = dryRun ? null : defaultOutput(path);
    let force = false;
    if (output && (await invoke<string>("path_kind", { path: output })) !== "missing") {
      const replace = await ask(`${output} exists. Replace it?`, { title: "Result file", okLabel: "Replace", cancelLabel: "Cancel" });
      if (!replace) return;
      force = true;
    }
    const design = clean(field("design").value) || null;
    if (!design && !(await confirmMidpoint(problem))) return;

    stopped = false;
    submit.disabled = true;
    stop.hidden = false;
    log.textContent = "";
    show(dryRun ? "Evaluating the start design…" : "Optimizing…");
    const onLine = new Channel<string>();
    onLine.onmessage = (line) => {
      // The status line shows the error, and the app already asked about the midpoint start.
      if (line.startsWith("error: ") || line.startsWith("warning: no --design given")) return;
      log.textContent += line + "\n";
      log.scrollTop = log.scrollHeight;
    };
    try {
      const request = {
        path,
        problem: adjust.checked ? problem : null,
        design,
        output,
        force,
        dry_run: dryRun,
      };
      const { outcome } = await invoke<{ outcome: Outcome }>("run_optimize", { request, onLine });
      if (outcome.exit_code === 0) {
        show(dryRun ? "Evaluated" : `Optimized, result in ${output}`);
      } else {
        show(stopped ? "Stopped" : (errorLine(outcome.stderr) ?? `clab exited with ${outcome.exit_code}`), false);
      }
    } catch (error) {
      show(String(error), false);
    } finally {
      stop.hidden = true;
      submit.disabled = false;
    }
  });
}
