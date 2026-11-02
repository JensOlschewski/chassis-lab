import { ask } from "@tauri-apps/plugin-dialog";
import { setupBrowse } from "./browse";
import { describe, run } from "./worker";

/** clab exit code: warnings need the user's confirmation. */
const CONFIRMATION_REQUIRED = 3;

/** Asks in a native dialog whether to proceed despite the warnings. */
function confirmWarnings(warnings: string[]): Promise<boolean> {
  return ask(warnings.join("\n\n"), { title: "Convert warnings", kind: "warning", okLabel: "Proceed", cancelLabel: "Cancel" });
}

interface ConvertRequest {
  input: string;
  domain: string;
  test: string | null;
  output: string | null;
  force: boolean;
  confirmed: boolean;
}

/** Reads the form into a request; empty optional fields become null. */
export function readRequest(form: HTMLFormElement): ConvertRequest {
  const data = new FormData(form);
  const text = (name: string) => String(data.get(name) ?? "").trim();
  const domain = text("domain");
  return {
    input: text("input"),
    domain,
    // Only knc has tests; clab rejects a test for svc.
    test: domain === "knc" ? text("test") : null,
    output: text("output") || null,
    force: data.has("force"),
    confirmed: false,
  };
}

/** The form as stored in a session file. */
export interface ConvertState {
  input: string | null;
  domain: string;
  test: string | null;
  output: string | null;
  force: boolean;
}

export function setupConvert() {
  const form = document.querySelector<HTMLFormElement>("#convert-form")!;
  const testField = document.querySelector<HTMLElement>("#test-field")!;
  const status = document.querySelector<HTMLElement>("#convert-status")!;
  const output = document.querySelector<HTMLElement>("#convert-output")!;
  const submit = form.querySelector<HTMLButtonElement>("button[type=submit]")!;

  const show = (message: string, ok: boolean, details = "") => {
    status.textContent = message;
    status.classList.toggle("error", !ok);
    output.textContent = details;
  };

  const syncTest = () => {
    testField.hidden = readRequest(form).domain !== "knc";
  };
  const field = (name: string) => form.elements.namedItem(name) as HTMLInputElement | HTMLSelectElement;

  form.addEventListener("change", syncTest);

  setupBrowse(form);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    submit.disabled = true;
    show("Converting…", true);
    try {
      const request = readRequest(form);
      if (!/\.(res|acd)$/i.test(request.input.replace(/["\s]+$/, ""))) {
        show("Input must be a .res or .acd file", false);
        return;
      }
      let outcome = await run("run_convert", request);
      if (outcome.exit_code === CONFIRMATION_REQUIRED) {
        const warnings = outcome.stderr
          .split("\n")
          .filter((line) => line.startsWith("warning: "))
          .map((line) => line.slice("warning: ".length));
        if (!(await confirmWarnings(warnings))) {
          show("Convert cancelled", false);
          return;
        }
        show("Converting…", true);
        outcome = await run("run_convert", { ...request, confirmed: true });
      }
      const { ok, message } = describe(outcome);
      show(message, ok, (outcome.stdout + outcome.stderr).trim());
    } catch (error) {
      show("Failed to run clab", false, String(error));
    } finally {
      submit.disabled = false;
    }
  });

  return {
    get(): ConvertState {
      const { input, confirmed, ...rest } = readRequest(form);
      return { input: input || null, ...rest };
    },
    set(state: ConvertState): void {
      field("input").value = state.input ?? "";
      field("domain").value = state.domain;
      if (state.test) field("test").value = state.test;
      field("output").value = state.output ?? "";
      (field("force") as HTMLInputElement).checked = state.force;
      syncTest();
      show("", true);
    },
  };
}
