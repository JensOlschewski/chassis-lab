import { Channel, invoke } from "@tauri-apps/api/core";
import { ask, message, save } from "@tauri-apps/plugin-dialog";
import { pick, setupBrowse } from "./browse";
import { Help, setupHelp } from "./help";
import {
  DesignValue,
  Section,
  createSection,
  decimalMessage,
  firstInvalid,
  invalidMessage,
  keptVariables,
  readParameters,
  setVariable,
  variableNames,
} from "./parameters";
import { element } from "./sections";
import { Outcome, errorLine } from "./worker";

/** Pasted paths often carry whitespace or the quotes added by "Copy as path". */
const clean = (path: string) => path.trim().replace(/^"|"$/g, "").trim();

/** `model.json` is solved to `model-solution.json` unless the user chooses another file. */
const defaultOutput = (model: string) => model.replace(/\.json$/i, "") + "-solution.json";

/** The parameter tables of the model, by the backend section `id`. Same shape as Optimize's SECTIONS. */
const SECTIONS: { id: string; subtitle?: string; help?: Help }[] = [
  {
    id: "hardpoints",
    subtitle: "The named points of the model",
    help: [
      ["X, Y, Z", "Coordinates of the point."],
      ["Design variable", "A quoted name for example \"DESIGN_VAR\" in a cell makes it a design variable. Value comes from design file"],
    ],
  },
  {
    id: "bushings",
    subtitle: "Stiffness and orientation of the bushings",
    help: [
      ["Active", "Switches the bushing on or off."],
      ["k1 … k6", "Stiffness in the six directions, or a stiffness file name for the whole bushing."],
      ["e 1 … 3", "Euler angles $\\varphi, \\theta, \\psi$ in degrees, z-x-z convention: $R = R_z(\\varphi)\\,R_x(\\theta)\\,R_z(\\psi)$ <a href=\"https://de.wikipedia.org/wiki/Eulersche_Winkel\">More Details</a>"],
    ],
  },
  {
    id: "forces",
    subtitle: "Springs between two points",
    help: [
      ["Active", "Switches the force element on or off."],
      ["Stiffness", "Linear spring stiffness, or the name of a stiffness JSON file."],
      ["Free length", "Length of the spring without load. Empty uses the distance between its two points at the start."],
    ],
  },
  {
    id: "drives",
    subtitle: "Forces, torques and motions over time",
    help: [
      ["x … rz", "Expression for one component over time. Use ~ for an unused component."],
      [
        "Expressions", "MATLAB expressions in $\\text{TIME}$ (or $\\text{time}$) \
        , For Example $sin\\left(\\frac{2\\pi\\,\\text{TIME}}{30}\\right)$"
      ]
    ],
  },
  {
    id: "simulation",
    subtitle: "Time range and step size of the simulation",
    help: [
      ["t_start", "First time point. Drive expressions see it as TIME."],
      ["t_end", "Last time point."],
      ["h", "Step size. The solver computes $(t_\\text{end}-t_\\text{start})/h+1$ time points."],
    ],
  },
];

export function setupSolve(openInInspect: (path: string) => void) {
  const form = document.querySelector<HTMLFormElement>("#solve-form")!;
  const designFields = document.querySelector<HTMLElement>("#solve-design")!;
  const designNote = document.querySelector<HTMLElement>("#solve-design-note")!;
  const saveButton = document.querySelector<HTMLElement>("#solve-save")!;
  const adjust = document.querySelector<HTMLInputElement>("#solve-adjust")!;
  const sections = document.querySelector<HTMLElement>("#solve-sections")!;
  const status = document.querySelector<HTMLElement>("#solve-status")!;
  const log = document.querySelector<HTMLElement>("#solve-log")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const stop = document.querySelector<HTMLButtonElement>("#solve-stop")!;
  const open = document.querySelector<HTMLButtonElement>("#solve-open")!;
  const installRuntime = document.querySelector<HTMLButtonElement>("#solve-install-runtime")!;

  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const setBusy = (busy: boolean) => {
    submit.disabled = busy;
    installRuntime.disabled = busy;
  };

  /** A failed run; a missing MATLAB Runtime also offers to install it. */
  const fail = (message: string) => {
    show(message, false);
    installRuntime.hidden = !message.startsWith("MATLAB Runtime");
  };

  const offerInstall = async () => {
    const proceed = await ask(
      "The solver needs the MATLAB Runtime. It downloads about 1.5 GB and installs for your user only. This can take several minutes.\n\nBy installing you accept the MathWorks MATLAB Runtime license.",
      { title: "Install MATLAB Runtime", okLabel: "Install", cancelLabel: "Cancel" },
    );
    if (!proceed) return;
    setBusy(true);
    log.textContent = "";
    show("Installing the MATLAB Runtime…");
    try {
      const outcome = await invoke<Outcome>("install_runtime");
      if (outcome.exit_code === 0) {
        installRuntime.hidden = true;
        show("MATLAB Runtime installed. Solve again.");
      } else {
        show(errorLine(outcome.stderr) ?? `clab exited with ${outcome.exit_code}`, false);
      }
    } catch (error) {
      show(String(error), false);
    } finally {
      setBusy(false);
    }
  };

  installRuntime.addEventListener("click", offerInstall);

  // Offer the install once, on the first launch that finds the solver without its Runtime.
  const offered = "runtimeOffered";
  if (!localStorage.getItem(offered)) {
    invoke<Outcome>("check_runtime")
      .then((outcome) => {
        if (outcome.exit_code === 0 || !errorLine(outcome.stderr)?.startsWith("MATLAB Runtime")) return;
        localStorage.setItem(offered, "1");
        installRuntime.hidden = false;
        return offerInstall();
      })
      .catch(() => { });
  }

  /** Values of a loaded design file; names without a cell (such as inside drive expressions) still reach the solver. */
  let loaded: Record<string, DesignValue> = {};

  // Number cells flag themselves; this reports whether any is left.
  sections.addEventListener("input", () => {
    const invalid = sections.querySelector(".invalid");
    show(invalid ? decimalMessage : "", !invalid);
  });

  const renderSections = (parameters: Section[]) => {
    const kept = keptVariables(sections);
    sections.replaceChildren(
      ...parameters.map((section) => createSection(section, kept, false, () => {
        const entry = SECTIONS.find((s) => s.id === section.id);
        if (entry?.help) showHelp(section.title, entry.subtitle, entry.help);
      })),
    );
    showDesign();
    designNote.textContent = "";
  };

  const showDesign = () => {
    designFields.hidden = !adjust.checked || !sections.hasChildNodes();
    saveButton.hidden = designFields.hidden;
  };
  adjust.addEventListener("change", showDesign);

  const loadModel = async () => {
    const model = clean(field("model").value);
    if (!model) return;
    try {
      renderSections(await invoke<Section[]>("model_parameters", { path: model }));
      show("");
    } catch (error) {
      renderSections([]);
      show(String(error), false);
    }
  };

  field("model").addEventListener("change", () => {
    fromOptimize = false;
    void loadModel();
  });

  /** Opened by Optimize's Model details: a cell named here reaches Optimize only through a saved model. Told once. */
  let fromOptimize = false;
  sections.addEventListener("change", () => {
    if (!fromOptimize || !sections.querySelector("input.named")) return;
    fromOptimize = false;
    void message("New design variables reach Optimize only after Save model… and loading the saved model in Optimize's Model files.", {
      title: "New design variable",
    });
  });
  setupBrowse(form);

  document.querySelector("#solve-load-design")!.addEventListener("click", async () => {
    const path = await pick(false, ["json"]);
    if (!path) return;
    try {
      loaded = await invoke<Record<string, DesignValue>>("read_design", { path });
      const names = variableNames(sections);
      names.forEach((name) => name in loaded && setVariable(sections, name, loaded[name]));
      const unused = Object.keys(loaded).filter((name) => !names.has(name)).length;
      const missing = [...names].filter((name) => !(name in loaded)).length;
      designNote.textContent = [
        `${names.size - missing} of ${names.size} variables set`,
        unused ? `${unused} in the file not used by the tables` : "",
      ]
        .filter(Boolean)
        .join(", ");
    } catch (error) {
      show(String(error), false);
    }
  });

  /** The model, design variable values and changed literals of the form, as the backend expects them. */
  const readCase = () => {
    const { design, overrides } = adjust.checked ? readParameters(sections) : { design: {}, overrides: {} };
    return { model: clean(field("model").value), design: adjust.checked ? { ...loaded, ...design } : {}, overrides };
  };

  /** Saves the adjusted model and, beside it, the design file that goes with it. */
  document.querySelector("#solve-save")!.addEventListener("click", async () => {
    const invalid = firstInvalid(sections);
    if (invalid) {
      invalid.focus();
      show(invalidMessage(invalid), false);
      return;
    }
    const defaultPath = clean(field("model").value).replace(/\.json$/i, "") + "-adjusted.json";
    const path = await save({ filters: [{ name: "json", extensions: ["json"] }], defaultPath });
    if (!path) return;
    try {
      const design = await invoke<string | null>("export_case", {
        case: readCase(),
        path,
      });
      show(design ? `Saved ${path} and ${design}` : `Saved ${path}`);
    } catch (error) {
      show(String(error), false);
    }
  });

  let stopped = false;
  stop.addEventListener("click", () => {
    stopped = true;
    void invoke("cancel_solve");
  });

  const showHelp = setupHelp(document.querySelector<HTMLDialogElement>("#solve-help")!);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const model = clean(field("model").value);
    // The solution goes beside the model.
    const output = defaultOutput(model);
    const invalid = adjust.checked ? firstInvalid(sections) : undefined;
    if (invalid) {
      invalid.focus();
      show(invalidMessage(invalid), false);
      return;
    }
    let force = false;
    if ((await invoke<string>("path_kind", { path: output })) !== "missing") {
      if (!(await ask(`${output} exists. Replace it?`, { title: "Solution file", okLabel: "Replace", cancelLabel: "Cancel" }))) return;
      force = true;
    }
    const request = { ...readCase(), output, force };
    stopped = false;
    setBusy(true);
    stop.hidden = false;
    open.hidden = true;
    installRuntime.hidden = true;
    log.textContent = "";
    show("Solving…");
    const onLine = new Channel<string>();
    onLine.onmessage = (line) => {
      // The status line shows the error.
      if (line.startsWith("error: ")) return;
      log.textContent += line + "\n";
      log.scrollTop = log.scrollHeight;
    };
    try {
      const outcome = await invoke<Outcome>("run_solve", { request, onLine });
      if (outcome.exit_code === 0) {
        show("Solved");
        open.onclick = () => openInInspect(output);
        open.hidden = false;
      } else {
        fail(stopped ? "Stopped" : (errorLine(outcome.stderr) ?? `clab exited with ${outcome.exit_code}`));
      }
    } catch (error) {
      fail(String(error));
    } finally {
      stop.hidden = true;
      setBusy(false);
    }
  });

  /** Loads `path` with its parameters shown, for another pane that wants the model's details. */
  return async (path: string) => {
    field("model").value = path;
    adjust.checked = true;
    fromOptimize = true;
    await loadModel();
    sections.querySelectorAll("details").forEach((details) => (details.open = true));
  };
}
