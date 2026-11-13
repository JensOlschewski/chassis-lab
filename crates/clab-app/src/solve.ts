import { Channel, invoke } from "@tauri-apps/api/core";
import { pick, setupBrowse } from "./browse";
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
import { BadgeState, createBadge, element, setBadge } from "./sections";
import { Outcome, errorLine } from "./worker";

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
  const adjust = document.querySelector<HTMLInputElement>("#solve-adjust")!;
  const sections = document.querySelector<HTMLElement>("#solve-sections")!;
  const status = document.querySelector<HTMLElement>("#solve-status")!;
  const steps = document.querySelector<HTMLElement>("#solve-steps")!;
  const summary = document.querySelector<HTMLElement>("#solve-summary")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;
  const stop = document.querySelector<HTMLButtonElement>("#solve-stop")!;

  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement;

  const show = (message: string, ok = true) => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
  };

  const setBusy = (busy: boolean) => {
    submit.disabled = busy;
  };

  /** Values of a loaded design file; names without a cell (such as inside drive expressions) still reach the solver. */
  let loaded: Record<string, DesignValue> = {};

  // Number cells flag themselves; this reports whether any is left.
  sections.addEventListener("input", () => {
    const invalid = sections.querySelector(".invalid");
    show(invalid ? decimalMessage : "", !invalid);
  });

  const renderSections = (parameters: Section[]) => {
    const kept = keptVariables(sections);
    sections.replaceChildren(...parameters.map((section) => createSection(section, kept, false)));
    showDesign();
    designNote.textContent = "";
  };

  const include = document.querySelector<HTMLElement>("#solve-include")!;
  const showDesign = () => {
    include.hidden = !adjust.checked;
    designFields.hidden = !adjust.checked || !sections.hasChildNodes();
  };
  adjust.addEventListener("change", showDesign);

  const loadModel = async () => {
    const model = clean(field("model").value);
    if (!model) return;
    try {
      renderSections(await invoke<Section[]>("model_parameters", { path: model }));
      if (!clean(field("output").value) || field("output").dataset.generated) {
        field("output").value = defaultOutput(model);
        field("output").dataset.generated = "true";
      }
      show("");
    } catch (error) {
      renderSections([]);
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
    const invalid = adjust.checked ? firstInvalid(sections) : undefined;
    if (invalid) {
      invalid.focus();
      show(invalidMessage(invalid), false);
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
